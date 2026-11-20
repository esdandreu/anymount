use std::sync::Mutex;

use crate::storages::google_drive::Http;

pub struct MockDriveHttp {
    pub calls: Mutex<Vec<String>>,
    pub writes: Mutex<Vec<(String, Vec<u8>)>>,
    pub responses: Mutex<Vec<(u16, Vec<u8>)>>,
}

impl Http for MockDriveHttp {
    fn request(
        &self,
        method: &str,
        url: &str,
        _headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, Vec<u8>), String> {
        self.calls.lock().expect("lock").push(url.into());
        self.writes
            .lock()
            .expect("lock")
            .push((method.into(), body.to_vec()));
        Ok(self.responses.lock().expect("lock").remove(0))
    }
}
