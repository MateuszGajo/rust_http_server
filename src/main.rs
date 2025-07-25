use std::{
    env,
    io::{ErrorKind, Read},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
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

fn handle_connection(stream: TcpStream, router: Router) {
    let mut stream = stream;
    let mut buffer = [0u8; 1024];
    stream.read(&mut buffer).unwrap();

    let mut request_obj = Parser::parse(&buffer);

    router.execute(&mut request_obj, &mut stream);
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
        let stream = TcpListener::bind(connection_string).unwrap();
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

#[cfg(test)]
mod tests {
    use crate::encoding::{Encoding, SupportedEncoding};

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

    #[test]
    fn test_echo_str() {
        start_test_server(ServerConfig::default());

        let mut stream = TcpStream::connect("127.0.0.1:4332").unwrap();
        let request = "GET /echo/test123 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
        assert!(buffer.contains("Content-Type: text/plain"));
        assert!(buffer.contains("test123"));
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

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 404 Not Found"));
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

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
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

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
        assert!(buffer.contains("Content-Type: text/plain"));
        assert!(buffer.contains("Content-Length: 12"));
        assert!(buffer.contains("foobar/1.2.3"));
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

                let mut buffer = String::new();
                stream
                    .read_to_string(&mut buffer)
                    .expect("Failed to read response");

                assert!(buffer.contains("HTTP/1.1 200 OK"));
                assert!(buffer.contains(&format!("thread-{}", i)));
            }));
        }
        for handle in handles {
            handle.join().expect("Thread panicked");
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

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();
        assert!(buffer.contains("HTTP/1.1 200 OK"));
        assert!(buffer.contains("Content-Type: application/octet-stream"));
        assert!(buffer.contains("Hello, world!"));

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

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();
        assert!(buffer.contains("HTTP/1.1 201 Created"));

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

        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).unwrap();

        let header_end = buffer
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("Invalid http response");

        let (header_bytes, body_bytes) = buffer.split_at(header_end + 4);

        let decoded_body = Encoding::decode("gzip", body_bytes.to_vec()).unwrap();

        let headers_str = String::from_utf8_lossy(header_bytes);
        let body_str = String::from_utf8_lossy(&decoded_body);

        assert!(headers_str.contains("HTTP/1.1 200 OK"));
        assert!(headers_str.contains("Content-Encoding: gzip"));
        assert!(body_str.contains("abc"));
    }
}
