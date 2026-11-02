//! Synthetic HTTP fixtures. Never talks to the user's server or Docker.
use crate::model::Config;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone)]
pub struct Request {
    pub path: String,
}
pub struct Response {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}
impl Response {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            body: value.to_string(),
            headers: vec![],
        }
    }
}
pub struct Mock {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<Request>>>,
}
impl Mock {
    pub fn new(handler: impl Fn(&Request) -> Response + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut one = [0];
                while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 65536 {
                    match stream.read(&mut one) {
                        Ok(0) => break,
                        Ok(_) => bytes.push(one[0]),
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                let header = String::from_utf8_lossy(&bytes);
                let request = Request {
                    path: header.split_whitespace().nth(1).unwrap_or("").to_owned(),
                };
                captured.lock().unwrap().push(request.clone());
                let response = handler(&request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nConnection: close\r\n",
                    response.status
                );
                if !response
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                {
                    let _ = write!(stream, "Content-Length: {}\r\n", response.body.len());
                }
                for (k, v) in response.headers {
                    let _ = write!(stream, "{k}: {v}\r\n");
                }
                let _ = write!(stream, "\r\n{}", response.body);
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
            requests,
        }
    }
    pub fn config(&self) -> Config {
        Config {
            endpoint: self.url.clone(),
            ..Default::default()
        }
    }
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
