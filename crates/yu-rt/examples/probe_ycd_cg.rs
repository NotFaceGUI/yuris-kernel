fn main() {
    for p in std::env::args().skip(1) {
        let bytes = std::fs::read(&p).expect("read");
        match yu_rt::ycd::YcdTable::parse(&bytes) {
            Ok(t) => {
                for c in &t.commands {
                    let want = std::env::var("YCD_CMD").unwrap_or_else(|_| "CG".into());
                    if c.name.eq_ignore_ascii_case(&want) {
                        let names: Vec<&str> = c.params.iter().map(|x| x.name.as_str()).collect();
                        println!("{p}\n  {} params: {}", names.len(), names.join(" "));
                    }
                }
            }
            Err(e) => println!("{p}: parse err {e:?}"),
        }
    }
}
