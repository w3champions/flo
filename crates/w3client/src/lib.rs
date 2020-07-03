#[test]
fn test_maps() {
  use casclib::open;
  let storage = open(r#"C:\Program Files (x86)\Warcraft III\Data"#).unwrap();

  dbg!(storage.file_count());

  // walk all files
  // for r in storage.files() {
  //   let entry = r.expect("file entry");
  //   let name = entry.get_name();
  //   dbg!(name);
  // }
}
