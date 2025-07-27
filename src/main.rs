use std::{
    env,
    io::{ErrorKind, Read},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    thread::{self},
    time::Duration,
};
mod encoding;
mod parser;
mod router;
use parser::Parser;
use router::Router;
use std::fs;
use std::sync::Arc;
use std::sync::Barrier;

use crate::{
    parser::Request,
    router::{Context, Response, Route},
};

fn is_http_version_supported(version: &str, expected_major: u32, expected_minior: u32) -> bool {
    if let Some(stripped) = version.strip_prefix("HTTP/") {
        let mut parts = stripped.split(".");
        if let (Some(major), Some(minior)) = (parts.next(), parts.next()) {
            if let (Ok(major), Ok(minior)) = (major.parse::<u32>(), minior.parse::<u32>()) {
                return (major > expected_major)
                    || (major == expected_major && minior >= expected_minior);
            }
        }
    }
    false
}

fn handle_connection(stream: TcpStream, router: Router) {
    let mut is_connection = true;

    while is_connection {
        let mut stream = stream.try_clone().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("Failed to set read timeout");
        let mut buffer = [0u8; 1024];

        let bytes_read = match stream.read(&mut buffer) {
            Ok(0) => {
                println!("Connection closed by client");
                break;
            }
            Ok(n) => n,
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                println!("Read timed out");
                break;
            }
            Err(e) => {
                eprintln!("Unexpected read error: {:?}", e);
                break;
            }
        };

        let mut request_obj = Parser::parse(&buffer[..bytes_read]);

        let mut is_connection_close = false;
        let is_persistent_connection = is_http_version_supported(&request_obj.version, 1, 1);
        let connection_close_header = request_obj.headers.get("Connection");
        if let Some(val) = connection_close_header {
            if val == "close" {
                is_connection_close = true;
            }
        }
        is_connection = is_persistent_connection && !is_connection_close;

        router.execute(&mut request_obj, &mut stream);
    }
}

pub struct HttpServer {
    handlers: Vec<Route>,
    directory: String,
}

type Job = Box<dyn FnOnce() + Send + 'static>;
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool {
            workers,
            sender: Some(sender),
        }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender.as_ref().unwrap().send(job).unwrap();
    }
}

struct Worker {
    id: usize,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let message = receiver.lock().unwrap().recv();

                match message {
                    Ok(job) => {
                        println!("Worker {id} got a job; executing.");

                        job();
                    }
                    Err(_) => {
                        println!("Worker {id} disconnected; shutting down.");
                        break;
                    }
                }
            }
        });

        Worker { id, thread }
    }
}
impl Drop for ThreadPool {
    fn drop(&mut self) {
        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);

            worker.thread.join().unwrap();
        }
    }
}
impl HttpServer {
    fn new(directory: String) -> Self {
        return HttpServer {
            handlers: Vec::new(),
            directory,
        };
    }
    fn listen(&self, connection_string: String, ready_barrier: Arc<Barrier>) {
        let stream = TcpListener::bind(connection_string).expect("Failed t obind tcp listner");
        ready_barrier.wait();

        let pool = ThreadPool::new(4);
        for con in stream.incoming() {
            let handlers = self.handlers.clone();
            let directory = self.directory.clone();
            pool.execute(move || {
                let router = Router::new(handlers, directory);
                handle_connection(con.unwrap(), router);
            });
        }
    }

    pub fn add_method(
        &mut self,
        method: String,
        path: String,
        callback: fn(&Request, &mut Response, &Context),
    ) {
        let route = Route {
            method,
            path,
            callback: callback,
        };
        let _ = &self.handlers.push(route);
    }
}

fn run_server(address: String, ready_barrier: Arc<Barrier>, directory_path: String) {
    let mut server = HttpServer::new(directory_path);
    server.add_method(String::from("GET"), String::from("/"), |_, response, _| {
        response.status(200).text(String::from("OK")).send();
    });

    server.add_method(
        String::from("GET"),
        String::from("/echo/{str}"),
        |req, response, _| {
            let str_param = match req.path_params.get("str") {
                Some(s) => s,
                None => return,
            };

            response.status(200).text(str_param.to_string()).send();
        },
    );
    server.add_method(
        String::from("GET"),
        String::from("/user-agent"),
        |req, response, _| {
            let user_agent = match req.headers.get("User-Agent") {
                Some(s) => s,
                None => return,
            };

            response.status(200).text(user_agent.to_string()).send();
        },
    );

    server.add_method(
        String::from("GET"),
        String::from("/files/{filename}"),
        |req, response, context| {
            let file_name = match req.path_params.get("filename") {
                Some(s) => s,
                None => return,
            };
            let full_path = format!("{}{}", context.directory_path, file_name);
            let data = match fs::read_to_string(full_path) {
                Ok(data) => data,
                Err(err) => {
                    if err.kind() == ErrorKind::NotFound {
                        return response.status(404).send();
                    } else {
                        return response
                            .status(500)
                            .text(String::from("Internal server error"))
                            .send();
                    }
                }
            };

            response
                .status(200)
                .text(data)
                .set_header(
                    String::from("Content-Type"),
                    String::from("application/octet-stream"),
                )
                .send();
        },
    );

    server.add_method(
        String::from("POST"),
        String::from("/files/{filename}"),
        |req, response, context| {
            let file_name = match req.path_params.get("filename") {
                Some(s) => s,
                None => return,
            };
            let full_path = format!("{}{}", context.directory_path, file_name);
            match fs::write(full_path, req.body.to_string()) {
                Ok(data) => data,
                Err(_) => {
                    return response
                        .status(500)
                        .text(String::from("Internal server error"))
                        .send();
                }
            };

            response.status(201).send();
        },
    );

    server.listen(address, ready_barrier);
}

fn main() {
    let argv = env::args().collect::<Vec<String>>();
    let mut argv = argv.iter();

    let mut directory_path: String = String::from("/");
    while let Some(arg) = argv.next() {
        if arg == "--directory" {
            if let Some(path) = argv.next() {
                directory_path = path.clone();
            }
            break;
        }
    }
    run_server(
        String::from("127.0.0.1:4221"),
        std::sync::Arc::new(std::sync::Barrier::new(1)),
        directory_path,
    );
}
// TODO: improve error handling
#[cfg(test)]
mod tests {

    use crate::encoding::Encoding;

    use super::*;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Barrier};
    use std::thread;

    struct ServerConfig {
        address: String,
        directory_path: String,
    }

    impl Default for ServerConfig {
        fn default() -> Self {
            ServerConfig {
                address: "127.0.0.1:4332".to_string(),
                directory_path: "/".to_string(),
            }
        }
    }
    fn start_test_server(config: ServerConfig) -> Arc<Barrier> {
        let ready = Arc::new(Barrier::new(2));
        let ready_clone = ready.clone();
        let addr = config.address.to_string();

        thread::spawn(move || {
            run_server(addr, ready_clone, config.directory_path);
        });

        ready.wait();

        ready
    }

    fn read_request(stream: &mut TcpStream) -> (String, String) {
        let mut buffer = Vec::new();
        let mut temp = [0; 1024];
        let header_end;
        loop {
            let n = stream.read(&mut temp).unwrap();
            if n == 0 {
                panic!("Connection closed before full response");
            }
            buffer.extend_from_slice(&temp[..n]);
            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }

        let (header_bytes, remaining) = buffer.split_at(header_end);
        let headers = String::from_utf8_lossy(header_bytes).into_owned();

        let content_length = headers
            .lines()
            .find(|line| line.to_lowercase().starts_with("content-length:"))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|val| val.trim().parse::<usize>().ok())
            .unwrap_or(0);

        let mut body = remaining.to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut temp).unwrap();
            if n == 0 {
                panic!("Connection closed before full body received");
            }
            body.extend_from_slice(&temp[..n]);
        }

        if headers.contains("Content-Encoding: gzip") {
            body = Encoding::decode("gzip", body.to_vec()).unwrap()
        }

        let body = String::from_utf8_lossy(&body).into_owned();

        return (headers, body);
    }

    #[test]
    fn test_echo_str() {
        start_test_server(ServerConfig::default());

        let mut stream = TcpStream::connect("127.0.0.1:4332").unwrap();

        let request = "GET /echo/test123 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Type: text/plain"));
        assert!(body.contains("test123"));

        drop(stream);
    }
    #[test]
    fn test_not_found() {
        let address = "127.0.0.1:4333";
        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /not/found HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, _) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 404 Not Found"));
        drop(stream);
    }

    #[test]
    fn test_ok_on_main_page() {
        let address = "127.0.0.1:4334";
        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, _) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        drop(stream);
    }

    #[test]
    fn test_user_agent() {
        let address = "127.0.0.1:4335";
        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request =
            "GET /user-agent HTTP/1.1\r\nHost: localhost\r\nUser-Agent: foobar/1.2.3\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Type: text/plain"));
        assert!(headers.contains("Content-Length: 12"));
        assert!(body.contains("foobar/1.2.3"));
    }

    #[test]
    fn test_concurrent_requests() {
        let address = "127.0.0.1:4336";
        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let num_threads = 10;
        let mut handles = vec![];

        for i in 0..num_threads {
            let address = address.to_string();
            handles.push(thread::spawn(move || {
                let mut stream = TcpStream::connect(&address).expect("Failed to connect");
                let request = format!("GET /echo/thread-{} HTTP/1.1\r\nHost: localhost\r\n\r\n", i);
                stream
                    .write_all(request.as_bytes())
                    .expect("Failed to write to stream");

                let (headers, _) = read_request(&mut stream);
                assert!(headers.contains("HTTP/1.1 200 OK"));
                assert!(headers.contains(&format!("thread-{}", i)));
                drop(stream)
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(_) => println!("ok"),
                Err(err) => println!("error {:?}", err),
            }
        }
    }

    #[test]
    fn test_file_handler() {
        let directory = "/tmp/";
        let file_name = "output";
        let file_path = format!("{}{}", directory, file_name);
        let address = "127.0.0.1:4337";

        start_test_server(ServerConfig {
            directory_path: directory.to_string(),
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut file = File::create(file_path).unwrap();

        file.write_all(b"Hello, world!").unwrap();

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /files/output HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);
        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Type: application/octet-stream"));
        assert!(body.contains("Hello, world!"));

        fs::remove_file("/tmp/output").unwrap()
    }

    #[test]
    fn test_file_adding_handler() {
        let directory = "/tmp/";
        let file_name = "newfile";
        let file_path = format!("{}{}", directory, file_name);
        let address = "127.0.0.1:4338";

        start_test_server(ServerConfig {
            directory_path: directory.to_string(),
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "POST /files/newfile HTTP/1.1\r\n\
        Host: localhost:4221\r\n\
        Content-Type: application/octet-stream\r\n\
        Content-Length: 5\r\n\
        \r\n\
        12345";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, _) = read_request(&mut stream);
        assert!(headers.contains("HTTP/1.1 201 Created"));

        let data = match fs::read_to_string(file_path) {
            Ok(data) => data,
            Err(err) => {
                panic!("error, {}", err)
            }
        };

        assert_eq!(data, "12345");
    }

    #[test]
    fn test_be_gzip_encoded() {
        let address = "127.0.0.1:4339";

        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /echo/abc HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Encoding: gzip"));
        assert!(headers.contains("Content-Length: 23"));
        assert!(body.contains("abc"));
    }

    #[test]
    fn test_multiple_compress_scheme() {
        let address = "127.0.0.1:4340";

        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /echo/abc HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: invalid-encoding-1, gzip, invalid-encoding-2\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Encoding: gzip"));
        assert!(headers.contains("Content-Length: 23"));
        assert!(body.contains("abc"));
    }

    #[test]
    fn test_persistatn_connection() {
        let address = "127.0.0.1:4341";

        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /echo/abc HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: invalid-encoding-1, gzip, invalid-encoding-2\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);
        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Content-Encoding: gzip"));
        assert!(headers.contains("Content-Length: 23"));
        assert!(body.contains("abc"));

        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("Failed to set read timeout");
        let request2 = "GET /echo/xyz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request2.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);

        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(body.contains("xyz"));
    }
    #[test]
    fn test_close_connection() {
        let address = "127.0.0.1:4342";

        start_test_server(ServerConfig {
            address: address.to_string(),
            ..ServerConfig::default()
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let request = "GET /echo/abc HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let (headers, body) = read_request(&mut stream);
        assert!(headers.contains("HTTP/1.1 200 OK"));
        assert!(headers.contains("Connection: close"));
        assert!(body.contains("abc"));

        let mut buf = [0u8; 1];

        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        match stream.peek(&mut buf) {
            Ok(0) => {}
            Ok(_) => {
                panic!("Expected connection to be closed, but it's still open (data available)");
            }
            Err(e) => {
                panic!(
                    "Expected connection to be closed, but got error instead: {}",
                    e
                );
            }
        }
    }
}
