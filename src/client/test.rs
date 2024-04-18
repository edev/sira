use super::*;
use std::fs;
use std::io::Write;

mod mktemp {
    use super::*;

    // We don't attempt to test the documented error conditions, because they should all be nearly
    // impossible in any sane environment, and these tests don't set up custom environments such as
    // Docker containers.

    #[test]
    fn file_and_path_match() -> anyhow::Result<()> {
        let test_string = "file and path must match";
        let (mut file, path) = mktemp()?;
        file.write_all(test_string.as_bytes())?;
        drop(file);
        let contents = fs::read_to_string(path)?;
        assert_eq!(contents, test_string);
        Ok(())
    }
}
