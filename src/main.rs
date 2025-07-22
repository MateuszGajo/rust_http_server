use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    thread,
};
mod parser;
use parser::Parser;
mod router;
use router::Router;
use std::sync::Arc;
use std::sync::Barrier;

use crate::{
    parser::Request,
    router::{Response, Route},
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
}

impl HttpServer {
    fn new() -> Self {
        return HttpServer {
            handlers: Vec::new(),
        };
    }
    fn listen(&self, connection_string: String) {
        let stream = TcpListener::bind(connection_string).unwrap();
        for con in stream.incoming() {
            let handlers = self.handlers.clone();
            thread::spawn(move || {
                let router = Router::new(handlers);
                handle_connection(con.unwrap(), router);
            });
        }
    }

    pub fn add_method(
        &mut self,
        method: String,
        path: String,
        callback: fn(&Request, &mut Response),
    ) {
        let route = Route {
            method,
            path,
            callback: callback,
        };
        let _ = &self.handlers.push(route);
    }
}

fn run_server(address: String, ready_barrier: Arc<Barrier>) {
    let mut server = HttpServer::new();
    server.add_method(String::from("GET"), String::from("/"), |_, response| {
        response.status(200).text(String::from("OK")).send();
    });

    server.add_method(
        String::from("GET"),
        String::from("/echo/{str}"),
        |req, response| {
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
        |req, response| {
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
        |req, response| {
            let file_name = match req.path_params.get("filename") {
                Some(s) => s,
                None => return,
            };
            // read file if exists return content if not return 404 Not Found

            // response.status(200).text(str_param.to_string()).send();
        },
    );

    ready_barrier.wait();
    server.listen(address);
}

fn main() {
    run_server(
        String::from("127.0.0.1:4221"),
        std::sync::Arc::new(std::sync::Barrier::new(1)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn start_test_server(addr: &str) -> Arc<Barrier> {
        let ready = Arc::new(Barrier::new(2));
        let ready_clone = ready.clone();
        let addr = addr.to_string();

        thread::spawn(move || {
            run_server(addr, ready_clone);
        });

        ready.wait();

        ready
    }

    #[test]
    fn test_echo_str() {
        start_test_server("127.0.0.1:4333");

        let mut stream = TcpStream::connect("127.0.0.1:4333").unwrap();
        let request = "GET /echo/test123 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
        assert!(buffer.contains("test123"));
    }
    #[test]
    fn test_not_found() {
        start_test_server("127.0.0.1:4333");

        let mut stream = TcpStream::connect("127.0.0.1:4333").unwrap();
        let request = "GET /not/found HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 404 Not Found"));
    }

    #[test]
    fn test_ok_on_main_page() {
        start_test_server("127.0.0.1:4333");

        let mut stream = TcpStream::connect("127.0.0.1:4333").unwrap();
        let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
    }

    #[test]
    fn test_user_agent() {
        start_test_server("127.0.0.1:4333");

        let mut stream = TcpStream::connect("127.0.0.1:4333").unwrap();
        let request =
            "GET /user-agent HTTP/1.1\r\nHost: localhost\r\nUser-Agent: foobar/1.2.3\r\n\r\n";
        stream.write_all(request.as_bytes()).unwrap();

        let mut buffer = String::new();
        stream.read_to_string(&mut buffer).unwrap();

        assert!(buffer.contains("HTTP/1.1 200 OK"));
        assert!(buffer.contains("foobar/1.2.3"));
    }

    #[test]
    fn test_concurrent_requests() {
        let address = "127.0.0.1:4334";
        start_test_server(address);

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
}
