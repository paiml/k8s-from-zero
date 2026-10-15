fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let (out, code) = kubelens::run(&args, &input);
    print!("{out}");
    std::process::exit(code);
}
