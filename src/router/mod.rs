use std::{collections::HashMap, io::Write, net::TcpStream};

use crate::encoding::Encoding;
use crate::parser::Request;

#[derive(Clone)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub callback: fn(&Request, &mut Response, &Context),
}

pub struct Context {
    pub directory_path: String,
}

pub struct Router {
    handlers: Vec<Route>,
    context: Context,
}

pub struct Response<'a> {
    status: i32,
    status_msg: String,
    text: String,
    socket: &'a mut TcpStream,
    protocol: String,
    headers: HashMap<String, String>,
    req_headers: HashMap<String, String>,
}

impl<'a> Response<'a> {
    pub fn new(
        socket: &'a mut TcpStream,
        protocol: String,
        req_headers: HashMap<String, String>,
    ) -> Self {
        Response {
            status: 200,
            text: String::new(),
            socket,
            protocol,
            status_msg: String::from("OK"),
            headers: HashMap::new(),
            req_headers,
        }
    }
    pub fn status(&mut self, status: i32) -> &mut Self {
        self.status = status;
        self.status_msg = match status {
            200 => String::from("OK"),
            201 => String::from("Created"),
            404 => String::from("Not Found"),
            _ => panic!("not handled status code"),
        };
        self
    }

    pub fn text(&mut self, text: String) -> &mut Self {
        self.text = text;
        self
    }

    pub fn set_header(&mut self, name: String, value: String) -> &mut Self {
        self.headers.insert(name, value);
        self
    }

    pub fn send(&mut self) {
        let mut text: Vec<u8> = self.text.clone().into_bytes();

        let accept_encoding = &self.req_headers.get("Accept-Encoding");

        if let Some((val, encoding)) = accept_encoding
            .as_ref()
            .and_then(|val| Encoding::select_encoding(val))
            .and_then(|val| Encoding::encode(val, text.clone()).ok())
        {
            text = val;
            self.headers
                .insert(String::from("Content-Encoding"), encoding.to_string());
        }
        if !text.is_empty() {
            if self.headers.get("Content-Type").is_none() {
                self.headers
                    .insert(String::from("Content-Type"), String::from("text/plain"));
            }
            self.headers
                .insert(String::from("Content-Length"), text.len().to_string());
        }
        let mut headers_resp = String::new();
        for (key, value) in &self.headers {
            headers_resp.push_str(&format!("{}: {}\r\n", key, value));
        }

        let response = self.build_response(&headers_resp, text);

        self.socket.write_all(&response).unwrap();
    }

    fn build_response(&self, headers_resp: &str, text: Vec<u8>) -> Vec<u8> {
        let status_line = format!("{} {} {}\r\n", self.protocol, self.status, self.status_msg);
        let headers = format!("{}\r\n", headers_resp);

        let mut response = Vec::new();
        response.extend_from_slice(status_line.as_bytes());
        response.extend_from_slice(headers.as_bytes());
        response.extend_from_slice(&text);

        response
    }
}

impl Router {
    pub fn new(handlers: Vec<Route>, directory_path: String) -> Self {
        Router {
            handlers,
            context: Context { directory_path },
        }
    }

    fn extract_path_params(template: String, path: String) -> Option<HashMap<String, String>> {
        let template_parts: Vec<&str> = template.trim_matches('/').split('/').collect();
        let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();

        if template_parts.len() != path_parts.len() {
            return None;
        }

        let mut params = HashMap::new();

        for (template, path) in template_parts.iter().zip(path_parts.iter()) {
            if template.starts_with('{') && template.ends_with('}') {
                let key = &template[1..&template.len() - 1];
                params.insert(key.to_string(), path.to_string());
            } else if template != path {
                return None;
            }
        }
        Some(params)
    }

    pub fn execute(&self, request: &mut Request, socket: &mut TcpStream) {
        let mut response = Response::new(socket, request.version.clone(), request.headers.clone());
        for route in &self.handlers {
            if let Some(path_params) =
                Router::extract_path_params(route.path.to_string(), request.path.to_string())
            {
                if route.method == request.method {
                    request.path_params = path_params;
                    (route.callback)(request, &mut response, &self.context);
                    return;
                };
            }
        }

        response.status(404).send();
    }
}
