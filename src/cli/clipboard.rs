use std::io::Read;

use crate::api::schema::{ClientClipboardSetParams, Method, Request};

pub(super) fn run_clipboard_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_clipboard_help();
        return Ok(2);
    };

    match subcommand {
        "set" => clipboard_set(&args[1..]),
        "help" | "--help" | "-h" => {
            print_clipboard_help();
            Ok(0)
        }
        _ => {
            print_clipboard_help();
            Ok(2)
        }
    }
}

fn clipboard_set(args: &[String]) -> std::io::Result<i32> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "help" | "--help" | "-h"))
    {
        print_clipboard_set_help();
        return Ok(0);
    }
    if args.len() != 1 || args[0] != "--stdin" {
        eprintln!("usage: herdr clipboard set --stdin");
        return Ok(2);
    }

    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;

    let response = super::send_request(&Request {
        id: "cli:clipboard:set".into(),
        method: Method::ClientClipboardSet(ClientClipboardSetParams { text }),
    })?;

    super::print_response(&response)
}

fn print_clipboard_help() {
    eprintln!("usage: herdr clipboard <command> [options]");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  set  Set foreground client clipboard content");
}

fn print_clipboard_set_help() {
    eprintln!("usage: herdr clipboard set --stdin");
    eprintln!();
    eprintln!("options:");
    eprintln!("  --stdin  Read clipboard text from stdin");
}
