use yu_rt::input::InputBinding;
fn main() {
    let t = yu_rt::sysvar_abi::SysVarTable::builtin_abi();
    let b = InputBinding::from_sysvars(&t);
    let z = t.decls.iter().position(|d| d.name == "_CODE_Z").unwrap() as u16;
    println!("_CODE_Z id={z} code_of_id={:?}", b.code_of_id(z));
    let enter = t
        .decls
        .iter()
        .position(|d| d.name == "_CODE_ENTER")
        .unwrap() as u16;
    println!(
        "_CODE_ENTER id={enter} code_of_id={:?}",
        b.code_of_id(enter)
    );
    println!("ok");
}
