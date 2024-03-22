use std::path::PathBuf;

/// Returns a [PathBuf] to the directory where Sira's configuration should live.
///
/// When compiled for testing, this returns `CARGO_MANIFEST_DIR` plus `resources/etc/sira`.
/// Otherwise, it returns `/etc/sira`.
pub fn config_dir() -> PathBuf {
    // Omit the leading slash so that PathBuf::push appends instead of replacing.
    const CONFIG_DIR: &str = "etc/sira";

    let mut path = PathBuf::new();

    #[cfg(test)]
    {
        path.push(env!("CARGO_MANIFEST_DIR"));
        path.push("resources");
    }

    path.push(CONFIG_DIR);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        let mut expected = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        expected.push("resources");
        expected.push("etc");
        expected.push("sira");

        assert_eq!(expected, config_dir());

        let config_dir_exists = expected.try_exists();
        assert!(config_dir_exists.expect("could not confirm or deny whether config dir exists"));
    }
}
