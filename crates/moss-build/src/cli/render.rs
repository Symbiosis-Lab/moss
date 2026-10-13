//! Newline-delimited JSON interface for rendering editor documents.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;

pub fn run(args: &[String]) -> i32 {
    if args != ["--json"] {
        eprintln!("Usage: moss render --json");
        eprintln!("Reads newline-delimited {{\"id\":number,\"source\":string}} requests from stdin.");
        return 2;
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    match run_stream(stdin.lock(), stdout.lock()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("moss render: {error}");
            1
        }
    }
}

fn run_stream<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    while let Some((line, oversized)) = read_request_line(&mut input)? {
        let response = if oversized {
            json!({ "id": null, "error": format!("request exceeds {MAX_REQUEST_BYTES} bytes") })
        } else {
            match serde_json::from_slice::<Value>(&line) {
                Ok(request) => render_request(request),
                Err(error) => json!({ "id": null, "error": format!("invalid JSON: {error}") }),
            }
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

fn read_request_line<R: BufRead>(input: &mut R) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() && !oversized {
                Ok(None)
            } else {
                Ok(Some((line, oversized)))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content_len = newline.unwrap_or(available.len());
        let room = MAX_REQUEST_BYTES.saturating_sub(line.len());
        let append_len = room.min(content_len);
        if append_len > 0 && !oversized {
            line.extend_from_slice(&available[..append_len]);
        }
        if content_len > append_len {
            oversized = true;
            line.clear();
        }
        input.consume(consumed);
        if newline.is_some() {
            return Ok(Some((line, oversized)));
        }
    }
}

fn render_request(request: Value) -> Value {
    let id = request.get("id").filter(|value| value.is_number()).cloned();
    let Some(source) = request.get("source").and_then(Value::as_str) else {
        return json!({ "id": id, "error": "request must contain a string source and numeric id" });
    };
    let Some(id) = id else {
        return json!({ "id": null, "error": "request must contain a string source and numeric id" });
    };
    let (html, css) = crate::build::markdown::render_standalone_document(source);
    json!({ "id": id, "html": html, "css": css })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_oversized_lines_do_not_stop_later_requests() {
        let mut input = b"{bad json}\n".to_vec();
        input.extend(std::iter::repeat_n(b'x', MAX_REQUEST_BYTES + 1));
        input.extend_from_slice(b"\n{\"id\":7,\"source\":\"# Ready\"}\n");
        let mut output = Vec::new();
        run_stream(input.as_slice(), &mut output).unwrap();
        let rows = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        assert!(rows[0]["error"].as_str().unwrap().starts_with("invalid JSON:"));
        assert!(rows[1]["error"].as_str().unwrap().contains("exceeds"));
        assert_eq!(rows[2]["id"], 7);
        assert!(rows[2]["html"].as_str().unwrap().contains("Ready"));
    }
}
