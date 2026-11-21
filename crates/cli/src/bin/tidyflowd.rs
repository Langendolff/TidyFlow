//! O mesmo `tidyflow`, sem janela de console no Windows: é o que sobe no login, o
//! menu de contexto e o navegador abrem.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    tidyflow::main()
}
