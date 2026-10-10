fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::io::stdin();
    let output = std::io::stdout();
    editor_core::table_interop::serve(input.lock(), output.lock())
}
