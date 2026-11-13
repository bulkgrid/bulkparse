//! Trusted local child process; never expose this protocol directly to a network.
#[path = "../protocol.rs"]
mod protocol;

use bulkparse::Error;
use protocol::{Reply, Request};
use std::io::{self, Read};

fn parse(request: Request) -> Reply {
    let id = request.id;
    let result = (|| {
        let file = std::fs::File::open(&request.path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::Malformed("input must be a regular file".into()));
        }
        let limit = request.options.limits.max_input_bytes;
        let mut bytes = Vec::new();
        file.take((limit as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        bulkparse::parse(
            &bytes,
            request.path.file_name().and_then(|s| s.to_str()),
            request.options,
        )
    })();
    match result {
        Ok(conversion) => Reply::Converted {
            id,
            conversion: Box::new(conversion),
        },
        Err(error) => Reply::Failed {
            id,
            code: error.code().into(),
            message: error.to_string(),
        },
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--max-response-bytes" {
        return Err("usage: bulkparse-worker --max-response-bytes N".into());
    }
    let limit: usize = args[1].parse()?;
    if !(1024..=protocol::MAX_RESPONSE).contains(&limit) {
        return Err("invalid response limit".into());
    }
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    protocol::write_frame(
        &mut output,
        &protocol::encode(
            &Reply::Ready {
                version: protocol::VERSION,
            },
            limit,
        )?,
    )?;
    loop {
        let frame = match protocol::read_frame(&mut input, protocol::MAX_REQUEST) {
            Ok(frame) => frame,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        let id = request.id;
        let reply = parse(request);
        let mut frame = match protocol::stage_reply(reply, limit) {
            Ok(frame) => frame,
            Err(error) => protocol::stage_reply(
                Reply::Failed {
                    id,
                    code: if error.kind() == io::ErrorKind::FileTooLarge {
                        "output_limit"
                    } else {
                        "io"
                    }
                    .into(),
                    message: format!(
                        "could not stage serialized conversion within response limit: {error}"
                    ),
                },
                limit,
            )?,
        };
        protocol::write_staged_frame(&mut output, &mut frame)?;
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("bulkparse-worker: {error}");
        std::process::exit(1);
    }
}
