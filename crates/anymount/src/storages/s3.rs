use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Component,
};

use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::{Client, primitives::ByteStream};
use serde::{Deserialize, Serialize};

use crate::domain::{
    ConnectStorageError, DirEntry, ReadDirError, ReadFileAtError, Storage,
    StorageConfig, StorageEntry, StoragePath, WriteAt, WriteFileError,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct S3Config {
    pub bucket: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub force_path_style: bool,
}

pub struct S3Storage {
    client: Client,
    bucket: String,
    prefix: String,
}

fn key(prefix: &str, path: &StoragePath) -> Result<String, String> {
    let mut parts = Vec::new();
    if !prefix.is_empty() {
        parts.push(prefix.to_owned());
    }
    for part in path.as_path().components() {
        if let Component::Normal(part) = part {
            parts.push(part.to_str().ok_or("path is not UTF-8")?.to_owned());
        }
    }
    Ok(parts.join("/"))
}

fn directory_prefix(key: &str) -> String {
    if key.is_empty() {
        String::new()
    } else {
        format!("{key}/")
    }
}

#[typetag::serde(name = "s3")]
#[async_trait::async_trait(?Send)]
impl StorageConfig for S3Config {
    async fn connect(&self) -> Result<Box<dyn Storage>, ConnectStorageError> {
        if self.bucket.is_empty() {
            return Err(ConnectStorageError::Failed {
                kind: "s3",
                message: "bucket must not be empty".into(),
            });
        }
        let prefix = self.prefix.trim_matches('/');
        if prefix
            .split('/')
            .any(|p| p == "." || p == ".." || p.is_empty())
            && !prefix.is_empty()
        {
            return Err(ConnectStorageError::Failed {
                kind: "s3",
                message: "prefix must contain normal path components".into(),
            });
        }
        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = &self.region {
            loader = loader.region(Region::new(region.clone()));
        }
        let shared = loader.load().await;
        let mut builder = aws_sdk_s3::config::Builder::from(&shared)
            .force_path_style(self.force_path_style);
        if let Some(endpoint) = &self.endpoint {
            builder = builder.endpoint_url(endpoint);
        }
        Ok(Box::new(S3Storage {
            client: Client::from_conf(builder.build()),
            bucket: self.bucket.clone(),
            prefix: prefix.to_owned(),
        }))
    }

    fn clone_config(&self) -> Box<dyn StorageConfig> {
        Box::new(self.clone())
    }
}

fn list_error(status: Option<u16>, message: String) -> ReadDirError {
    match status {
        Some(401 | 403) => ReadDirError::PermissionDenied,
        Some(404) => ReadDirError::NotFound,
        Some(429 | 500..=599) | None => ReadDirError::Unavailable { message },
        _ => ReadDirError::Unknown { message },
    }
}

fn read_error(status: Option<u16>, message: String) -> ReadFileAtError {
    match status {
        Some(401 | 403) => ReadFileAtError::PermissionDenied,
        Some(404) => ReadFileAtError::NotFound,
        Some(416) => ReadFileAtError::RangeNotSatisfiable,
        Some(429 | 500..=599) | None => {
            ReadFileAtError::Unavailable { message }
        }
        _ => ReadFileAtError::Unknown { message },
    }
}

fn write_error(status: Option<u16>, message: String) -> WriteFileError {
    match status {
        Some(401 | 403) => WriteFileError::PermissionDenied,
        Some(404) => WriteFileError::NotFound,
        Some(429 | 500..=599) | None => WriteFileError::Unavailable { message },
        _ => WriteFileError::Failed { message },
    }
}

fn content_range_end(
    value: Option<&str>,
    expected_start: u64,
    requested_end: u64,
) -> Result<u64, ReadFileAtError> {
    let invalid = || ReadFileAtError::Unknown {
        message: "S3 returned an invalid content range".into(),
    };
    let (range, total) = value
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('/'))
        .ok_or_else(invalid)?;
    let (start, end) = range.split_once('-').ok_or_else(invalid)?;
    let start: u64 = start.parse().map_err(|_| invalid())?;
    let end: u64 = end.parse().map_err(|_| invalid())?;
    let total: u64 = total.parse().map_err(|_| invalid())?;
    if start != expected_start
        || end < start
        || end >= requested_end
        || end >= total
        || (end + 1 < requested_end && end + 1 != total)
    {
        return Err(invalid());
    }
    Ok(end)
}

#[async_trait::async_trait(?Send)]
impl Storage for S3Storage {
    async fn read_dir(
        &self,
        path: StoragePath,
    ) -> Result<Vec<Box<dyn DirEntry>>, ReadDirError> {
        let prefix = directory_prefix(
            &key(&self.prefix, &path)
                .map_err(|message| ReadDirError::Unknown { message })?,
        );
        let mut token = None;
        let mut seen_tokens = BTreeSet::new();
        let mut marker_exists = false;
        let mut entries = BTreeMap::<String, StorageEntry>::new();
        loop {
            let page = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .delimiter("/")
                .set_continuation_token(token)
                .send()
                .await
                .map_err(|err| {
                    list_error(
                        err.raw_response().map(|r| r.status().as_u16()),
                        err.to_string(),
                    )
                })?;
            for object in page.contents() {
                marker_exists |= object.key() == Some(prefix.as_str());
                if let Some(name) = object
                    .key()
                    .and_then(|k| k.strip_prefix(&prefix))
                    .filter(|n| !n.is_empty() && !n.contains('/'))
                {
                    entries.insert(
                        name.to_owned(),
                        StorageEntry {
                            name: name.to_owned(),
                            is_directory: false,
                            size: object
                                .size()
                                .and_then(|n| u64::try_from(n).ok()),
                            accessed: None,
                        },
                    );
                }
            }
            for directory in page.common_prefixes() {
                if let Some(name) = directory
                    .prefix()
                    .and_then(|p| p.strip_prefix(&prefix))
                    .and_then(|p| p.strip_suffix('/'))
                    .filter(|p| !p.is_empty() && !p.contains('/'))
                {
                    entries.insert(
                        name.to_owned(),
                        StorageEntry {
                            name: name.to_owned(),
                            is_directory: true,
                            size: None,
                            accessed: None,
                        },
                    );
                }
            }
            if !page.is_truncated().unwrap_or(false) {
                break;
            }
            token = page.next_continuation_token().map(str::to_owned);
            match token.as_ref() {
                Some(next) if !next.is_empty() && seen_tokens.insert(next.clone()) => {},
                _ => return Err(ReadDirError::Unknown {
                    message: "S3 listing returned a missing or repeated continuation token".into(),
                }),
            }
        }
        if path != StoragePath::root() && entries.is_empty() && !marker_exists {
            let object_key = key(&self.prefix, &path)
                .map_err(|message| ReadDirError::Unknown { message })?;
            match self
                .client
                .head_object()
                .bucket(&self.bucket)
                .key(object_key)
                .send()
                .await
            {
                Ok(_) => return Err(ReadDirError::NotADirectory),
                Err(err)
                    if err.raw_response().map(|r| r.status().as_u16())
                        == Some(404) =>
                {
                    return Err(ReadDirError::NotFound);
                }
                Err(err) => {
                    return Err(list_error(
                        err.raw_response().map(|r| r.status().as_u16()),
                        err.to_string(),
                    ));
                }
            }
        }
        Ok(entries
            .into_values()
            .map(|entry| Box::new(entry) as Box<dyn DirEntry>)
            .collect())
    }

    async fn read_file_at(
        &self,
        path: StoragePath,
        writer: &mut dyn WriteAt,
        range: Range<u64>,
    ) -> Result<u64, ReadFileAtError> {
        if range.start > range.end {
            return Err(ReadFileAtError::Unknown {
                message: "invalid range".into(),
            });
        }
        if range.start == range.end {
            return Ok(0);
        }
        let key = key(&self.prefix, &path)
            .map_err(|message| ReadFileAtError::Unknown { message })?;
        if key.is_empty() || path == StoragePath::root() {
            return Err(ReadFileAtError::NotAFile);
        }
        let mut offset = range.start;
        while offset < range.end {
            let end = offset
                + (range.end - offset).min(crate::domain::MAX_READ_SIZE as u64);
            let response = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(&key)
                .range(format!("bytes={offset}-{}", end - 1))
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(err)
                    if err.raw_response().map(|r| r.status().as_u16())
                        == Some(416) =>
                {
                    break;
                }
                Err(err) => {
                    return Err(read_error(
                        err.raw_response().map(|r| r.status().as_u16()),
                        err.to_string(),
                    ));
                }
            };
            let returned_end =
                content_range_end(response.content_range(), offset, end)?;
            let expected = (returned_end - offset + 1) as usize;
            let mut body = response.body;
            let mut bytes = Vec::with_capacity(expected);
            while let Some(chunk) = body.next().await {
                let chunk =
                    chunk.map_err(|err| ReadFileAtError::Unavailable {
                        message: err.to_string(),
                    })?;
                if chunk.len() > expected - bytes.len() {
                    return Err(ReadFileAtError::Unknown {
                        message: "S3 returned more data than its content range"
                            .into(),
                    });
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.len() != expected {
                return Err(ReadFileAtError::Unknown {
                    message: "S3 returned less data than its content range"
                        .into(),
                });
            }
            writer.write_at(&bytes, offset)?;
            offset += bytes.len() as u64;
            if offset < end {
                break;
            }
        }
        Ok(offset - range.start)
    }

    async fn write_file(
        &self,
        path: StoragePath,
        bytes: &[u8],
    ) -> Result<(), WriteFileError> {
        if path == StoragePath::root() {
            return Err(WriteFileError::NotAFile);
        }
        let key = key(&self.prefix, &path)
            .map_err(|message| WriteFileError::Failed { message })?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|err| {
                write_error(
                    err.raw_response().map(|r| r.status().as_u16()),
                    err.to_string(),
                )
            })?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn keys_remain_inside_configured_prefix() {
        let path = StoragePath::try_from(PathBuf::from("hello world/é.txt"))
            .expect("valid path");
        assert_eq!(
            key("root", &path).expect("UTF-8"),
            "root/hello world/é.txt"
        );
        assert_eq!(
            directory_prefix(&key("root", &StoragePath::root()).expect("root")),
            "root/"
        );
        assert!(StoragePath::try_from(PathBuf::from("../escape")).is_err());
    }

    #[test]
    fn maps_s3_statuses_to_storage_errors() {
        assert!(matches!(
            list_error(Some(403), String::new()),
            ReadDirError::PermissionDenied
        ));
        assert!(matches!(
            read_error(Some(404), String::new()),
            ReadFileAtError::NotFound
        ));
        assert!(matches!(
            write_error(Some(503), String::new()),
            WriteFileError::Unavailable { .. }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deserializes_registered_config_and_rejects_empty_bucket() {
        let config: Box<dyn StorageConfig> =
            toml::from_str("type = 's3'\nbucket = ''").expect("config");
        assert!(matches!(
            config.connect().await,
            Err(ConnectStorageError::Failed { kind: "s3", .. })
        ));
    }
    fn serve(
        responses: Vec<(u16, Vec<u8>, Option<&'static str>)>,
    ) -> (String, std::thread::JoinHandle<Vec<Vec<u8>>>) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind test server");
        let endpoint = format!(
            "http://{}",
            listener.local_addr().expect("server address")
        );
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body, content_range) in responses {
                let (mut stream, _) =
                    listener.accept().expect("accept request");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .expect("read timeout");
                let mut data = Vec::new();
                let mut buffer = [0; 8192];
                let headers_end = loop {
                    let count = stream.read(&mut buffer).expect("read request");
                    assert_ne!(count, 0, "request ended before headers");
                    data.extend_from_slice(&buffer[..count]);
                    if let Some(pos) =
                        data.windows(4).position(|part| part == b"\r\n\r\n")
                    {
                        break pos + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&data[..headers_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        if name.eq_ignore_ascii_case("content-length") {
                            value.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                while data.len() < headers_end + length {
                    let count =
                        stream.read(&mut buffer).expect("read request body");
                    assert_ne!(count, 0, "request ended before body");
                    data.extend_from_slice(&buffer[..count]);
                }
                requests.push(data);
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n",
                    body.len()
                )
                .expect("response headers");
                if let Some(range) = content_range {
                    write!(stream, "Content-Range: {range}\r\n")
                        .expect("range header");
                }
                write!(stream, "Connection: close\r\n\r\n")
                    .expect("response headers");
                stream.write_all(&body).expect("response body");
            }
            requests
        });
        (endpoint, handle)
    }

    fn test_storage(endpoint: String) -> S3Storage {
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                "key", "secret", None, None, "test",
            ))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .build();
        S3Storage {
            client: Client::from_conf(config),
            bucket: "bucket".into(),
            prefix: "root".into(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lists_all_pages_and_decodes_entries() {
        let first = br#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><IsTruncated>true</IsTruncated><Contents><Key>root/one.txt</Key><Size>3</Size></Contents><NextContinuationToken>A&amp;B</NextContinuationToken></ListBucketResult>"#.to_vec();
        let second = br#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><IsTruncated>false</IsTruncated><CommonPrefixes><Prefix>root/folder/</Prefix></CommonPrefixes></ListBucketResult>"#.to_vec();
        let (endpoint, server) =
            serve(vec![(200, first, None), (200, second, None)]);
        let entries = test_storage(endpoint)
            .read_dir(StoragePath::root())
            .await
            .expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name(), "folder");
        assert!(entries[0].is_dir());
        assert_eq!(entries[1].file_name(), "one.txt");
        assert_eq!(entries[1].size(), Some(3));
        let requests = server.join().expect("server");
        let first = String::from_utf8_lossy(&requests[0]);
        let second = String::from_utf8_lossy(&requests[1]);
        assert!(first.contains("list-type=2"));
        assert!(first.contains("prefix=root%2F"));
        assert!(first.contains("delimiter=%2F"));
        assert!(second.contains("continuation-token=A%26B"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_large_ranges_in_bounded_requests() {
        let first = vec![b'a'; crate::domain::MAX_READ_SIZE];
        let (endpoint, server) = serve(vec![
            (206, first, Some("bytes 10-65545/65549")),
            (206, b"xyz".to_vec(), Some("bytes 65546-65548/65549")),
        ]);
        let storage = test_storage(endpoint);
        let path =
            StoragePath::try_from(PathBuf::from("file.txt")).expect("path");
        let mut writer = crate::test_utils::mocks::BytesWriter::default();
        let count = storage
            .read_file_at(
                path,
                &mut writer,
                10..(10 + crate::domain::MAX_READ_SIZE as u64 + 3),
            )
            .await
            .expect("read");
        assert_eq!(count, crate::domain::MAX_READ_SIZE as u64 + 3);
        assert_eq!(&writer.0[crate::domain::MAX_READ_SIZE..], b"xyz");
        let requests = server.join().expect("server");
        let first = String::from_utf8_lossy(&requests[0]).to_ascii_lowercase();
        let second = String::from_utf8_lossy(&requests[1]).to_ascii_lowercase();
        assert!(first.starts_with("get /bucket/root/file.txt"));
        assert!(first.contains("range: bytes=10-65545"));
        assert!(second.contains("range: bytes=65546-65548"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_range_response_for_different_offset() {
        let (endpoint, server) =
            serve(vec![(206, b"wrong".to_vec(), Some("bytes 0-4/100"))]);
        let storage = test_storage(endpoint);
        let path =
            StoragePath::try_from(PathBuf::from("file.txt")).expect("path");
        let mut writer = crate::test_utils::mocks::BytesWriter::default();
        assert!(
            storage
                .read_file_at(path, &mut writer, 10..15)
                .await
                .is_err()
        );
        assert!(writer.0.is_empty());
        server.join().expect("server");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn writes_object_to_prefixed_key() {
        let (endpoint, server) = serve(vec![(200, Vec::new(), None)]);
        let storage = test_storage(endpoint);
        let path = StoragePath::try_from(PathBuf::from("hello world.txt"))
            .expect("path");
        storage.write_file(path, b"hello s3").await.expect("write");
        let requests = server.join().expect("server");
        let request = String::from_utf8_lossy(&requests[0]);
        assert!(request.starts_with("PUT /bucket/root/hello%20world.txt"));
        assert!(requests[0].ends_with(b"hello s3"));
    }
}
