use flate2::Compression;
use flate2::bufread::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{self};
use std::io::{Read, Write};
use std::str::FromStr;
pub struct Encoding;

#[derive(Debug, PartialEq)]
pub enum SupportedEncoding {
    Gzip,
}

impl FromStr for SupportedEncoding {
    type Err = ();

    fn from_str(input: &str) -> Result<SupportedEncoding, Self::Err> {
        match input {
            "gzip" => Ok(SupportedEncoding::Gzip),
            _ => Err(()),
        }
    }
}
impl Encoding {
    pub fn encode(
        encoding_type: &str,
        data: Vec<u8>,
    ) -> Result<(Vec<u8>, &'static str), io::Error> {
        match SupportedEncoding::from_str(encoding_type) {
            Ok(SupportedEncoding::Gzip) => {
                let encoded = Encoding::gzip_encode(data)?;
                Ok((encoded, "gzip"))
            }
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported encoding",
            )),
        }
    }

    pub fn decode(encoding_type: &str, compressed: Vec<u8>) -> Result<Vec<u8>, io::Error> {
        match SupportedEncoding::from_str(encoding_type) {
            Ok(SupportedEncoding::Gzip) => Encoding::gzip_decode(compressed),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unsupported encoding",
            )),
        }
    }

    fn gzip_encode(data: Vec<u8>) -> Result<Vec<u8>, io::Error> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write(&data)?;
        encoder.finish()
    }

    fn gzip_decode(data: Vec<u8>) -> Result<Vec<u8>, io::Error> {
        let mut decoder = GzDecoder::new(&data[..]);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output)?;
        Ok(output)
    }
}
