use std::{io::{Read, Write}, net::{TcpListener, TcpStream}};
mod parser;
use parser::Parser;
mod router;
use router::Router;

fn handle_connection(stream: TcpStream, router: &Router) {
    let mut stream = stream;
    let mut buffer = [0u8; 1024];
    stream.read(&mut buffer).unwrap();


    let request_obj = Parser::parse(&buffer);

    router.execute(&request_obj, &mut stream);
}

pub struct HttpServer {
    router: Router
}

impl HttpServer {
    fn new() -> Self{
        let router = Router::new();
        HttpServer { router }
    }

    fn listen(&self, connection_string: String) {
        let stream = TcpListener::bind(connection_string).unwrap();

        for con in stream.incoming() {
                handle_connection(con.unwrap(), &self.router);
            }
    }
}

fn main() {
    let mut server = HttpServer::new();
 
    server.router.add_method(String::from("GET"), String::from("/"), |req, response| {
        response.status(200).text(String::from("OK")).send();
    });

    server.listen(String::from("127.0.0.1:4221"));
  
  
}

