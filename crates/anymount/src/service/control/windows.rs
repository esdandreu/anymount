//! Named-pipe control transport for provider services on Windows.

use crate::service::control::messages::ControlMessage;
use crate::service::control::paths::provider_endpoint;
use crate::service::{Error, Result};
use std::fs;
use std::io;
use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, NAMED_PIPE_MODE,
    PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsControl;

impl WindowsControl {
    pub fn bind(&self, provider_name: &str) -> Result<WindowsPipeListener> {
        let _ = ensure_pipe_state_path(provider_name)?;
        Ok(WindowsPipeListener {
            pipe_name: pipe_hstring(provider_name)?,
        })
    }

    pub fn send(&self, provider_name: &str, message: ControlMessage) -> Result<ControlMessage> {
        let _ = ensure_pipe_state_path(provider_name)?;
        let name = pipe_hstring(provider_name)?;
        let handle = connect_client_pipe(&name, provider_name)?;
        let encoded = message.encode();
        write_all_pipe(handle, provider_name, &encoded)?;
        unsafe {
            let _ = FlushFileBuffers(handle);
        }
        let reply_bytes = read_message_pipe(handle, provider_name)?;
        unsafe {
            let _ = DisconnectNamedPipe(handle);
            let _ = CloseHandle(handle);
        }
        ControlMessage::decode(&reply_bytes)
    }
}

fn ensure_pipe_state_path(provider_name: &str) -> Result<std::path::PathBuf> {
    let path = provider_endpoint(provider_name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            operation: "create pipe state directory",
            provider_name: provider_name.to_owned(),
            source,
        })?;
    }
    Ok(path)
}

fn pipe_hstring(provider_name: &str) -> Result<HSTRING> {
    provider_endpoint(provider_name)?;
    Ok(HSTRING::from(format!(r"\\.\pipe\anymount-{provider_name}")))
}

/// Server side: blocks for one client, one request/response cycle.
pub struct WindowsPipeListener {
    pipe_name: HSTRING,
}

impl WindowsPipeListener {
    pub fn serve_one_exchange(
        &self,
        provider_name: &str,
        handle_control: impl FnOnce(&[u8]) -> (ControlMessage, bool),
    ) -> Result<bool> {
        let pipe = create_server_pipe(&self.pipe_name, provider_name)?;
        unsafe {
            ConnectNamedPipe(pipe, None)
                .map_err(|e| win_to_io_error("ConnectNamedPipe", provider_name, e))?;
        }
        let request = read_message_pipe(pipe, provider_name)?;
        let (reply, stop) = handle_control(&request);
        let encoded = reply.encode();
        write_all_pipe(pipe, provider_name, &encoded)?;
        unsafe {
            let _ = FlushFileBuffers(pipe);
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
        Ok(stop)
    }
}

fn create_server_pipe(name: &HSTRING, provider_name: &str) -> Result<HANDLE> {
    let mode: NAMED_PIPE_MODE = PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT;
    let handle = unsafe {
        CreateNamedPipeW(
            name,
            PIPE_ACCESS_DUPLEX,
            mode,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            None,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io_error(
            "CreateNamedPipeW",
            provider_name,
            io::Error::from_raw_os_error(unsafe { GetLastError().0 as i32 }),
        ));
    }
    Ok(handle)
}

fn connect_client_pipe(name: &HSTRING, provider_name: &str) -> Result<HANDLE> {
    let share = FILE_SHARE_MODE(0);
    let handle = unsafe {
        CreateFileW(
            name,
            GENERIC_READ.0 | GENERIC_WRITE.0,
            share,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            HANDLE(std::ptr::null_mut()),
        )
    }
    .map_err(|e| win_to_io_error("CreateFileW pipe client", provider_name, e))?;
    Ok(handle)
}

fn write_all_pipe(handle: HANDLE, provider_name: &str, data: &[u8]) -> Result<()> {
    let mut total = 0u32;
    while (total as usize) < data.len() {
        let mut written = 0u32;
        unsafe {
            WriteFile(
                handle,
                Some(&data[total as usize..]),
                Some(&mut written),
                None,
            )
            .map_err(|e| win_to_io_error("WriteFile", provider_name, e))?;
        }
        if written == 0 {
            return Err(io_error(
                "WriteFile",
                provider_name,
                io::Error::new(io::ErrorKind::WriteZero, "WriteFile wrote 0 bytes"),
            ));
        }
        total += written;
    }
    Ok(())
}

fn read_message_pipe(handle: HANDLE, provider_name: &str) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; 4096];
    let mut n = 0u32;
    unsafe {
        ReadFile(handle, Some(&mut buf), Some(&mut n), None)
            .map_err(|e| win_to_io_error("ReadFile", provider_name, e))?;
    }
    buf.truncate(n as usize);
    Ok(buf)
}

fn io_error(operation: &'static str, provider_name: &str, source: io::Error) -> Error {
    Error::Io {
        operation,
        provider_name: provider_name.to_owned(),
        source,
    }
}

fn win_to_io_error(operation: &'static str, provider_name: &str, e: windows::core::Error) -> Error {
    io_error(operation, provider_name, io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::control::messages::ControlMessage;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn named_pipe_ping_and_shutdown_round_trip() {
        let control = WindowsControl;
        let listener = control.bind("winpipe-test").expect("bind listener");
        let pipe_name = pipe_hstring("winpipe-test").expect("name");

        let server = thread::spawn(move || {
            for _ in 0..2 {
                let stop = listener
                    .serve_one_exchange("winpipe-test", |bytes| {
                        match ControlMessage::decode(bytes).expect("decode") {
                            ControlMessage::Ping => (ControlMessage::Ready, false),
                            ControlMessage::Shutdown => (ControlMessage::Ack, true),
                            other => panic!("unexpected {other:?}"),
                        }
                    })
                    .expect("serve");
                if stop {
                    break;
                }
            }
        });

        thread::sleep(Duration::from_millis(50));
        let r1 = control
            .send("winpipe-test", ControlMessage::Ping)
            .expect("ping");
        assert_eq!(r1, ControlMessage::Ready);
        let r2 = control
            .send("winpipe-test", ControlMessage::Shutdown)
            .expect("shutdown");
        assert_eq!(r2, ControlMessage::Ack);

        server.join().expect("server join");
    }
}
