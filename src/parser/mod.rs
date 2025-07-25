use std::collections::HashMap;

pub struct Parser;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub body: String,
}

impl Parser {
    pub fn parse(buff: &[u8]) -> Request {
        let text = std::str::from_utf8(&buff).expect("invalid msg");

        let mut lines = text.split("\r\n");

        let request_line = lines.next().ok_or("no header").unwrap();

        let mut request_line = request_line.split_whitespace();

        let method = request_line.next().ok_or("missing method").unwrap();
        let path = request_line.next().ok_or("missing path").unwrap();
        let version = request_line.next().ok_or("missing version").unwrap();
        println!(
            "alright, method {}, path: {} and version:{}",
            method, path, version
        );

        let mut headers: HashMap<String, String> = HashMap::new();
        while let Some(line) = lines.next() {
            if line.is_empty() {
                break;
            }

            let mut lines = line.split(":");

            let header_name = lines.next().unwrap();
            let header_val = lines.next().unwrap().trim();

            headers.insert(header_name.to_string(), header_val.to_string());

            println!("Header: {}", line)
        }

        let mut body = String::new();
        let content_length: usize = headers
            .get("Content-Length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if let Some(line) = lines.next() {
            body = line[0..content_length].to_string()
        }

        let request = Request {
            version: version.to_string(),
            method: method.to_string(),
            path: path.to_string(),
            path_params: HashMap::new(),
            headers,
            body,
        };

        request
    }
}
