use crate::cli::Cmd;
use crate::exit::Exit;

pub fn not_yet(cmd: &Cmd) -> Exit {
    let (name, iteration) = cmd.stub_info().unwrap_or(("this command", 0));
    eprintln!("error: `yojaka {name}` arrives in Iteration {iteration}");
    eprintln!("hint: this is the pre-alpha skeleton; see all-specs/03-iteration-plan.md");
    Exit::Error
}
