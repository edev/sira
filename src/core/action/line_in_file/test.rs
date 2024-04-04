use super::*;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

// Specifies a test case with a minimum of boilerplate.
struct Case {
    // The simulated contents of the file at Action::LineInFile::path.
    contents: &'static str,

    // The value of Action::LineInFile::line.
    line: &'static str,

    // The value of Action::LineInFile::pattern.
    pattern: Option<&'static str>,

    // The value of Action::LineInFile::after.
    after: Option<&'static str>,

    // The value of Action::LineInFile::indent.
    indent: bool,

    // The expected final state of the file.
    expected: &'static str,
}

macro_rules! check {
    ($case:expr) => {
        // Create a temporary file containing `$case.contents`.
        let temp_file = NamedTempFile::new().unwrap();
        let (mut temp_file, path) = temp_file.keep().unwrap();
        let path = path.to_string_lossy().to_string();
        temp_file.write_all(&$case.contents.as_bytes()).unwrap();
        drop(temp_file);

        let action = Action::LineInFile {
            path: path.clone(),
            line: $case.line.to_string(),
            pattern: $case.pattern.map(str::to_string),
            after: $case.after.map(str::to_string),
            indent: $case.indent,
        };
        line_in_file(&action).unwrap();

        // Read the temporary file's contents, then clean up the file before we assert anything.
        let file = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!($case.expected, file);
    };
}

#[test]
#[should_panic(expected = "not a LineInFile")]
fn if_action_is_wrong_type_panics() {
    line_in_file(&Action::Shell(vec!["uh-oh".to_string()])).unwrap();
}

#[test]
fn order_of_operations() {
    // Tests the order of operations:

    // 1. If line is present, no change.
    check!(Case {
        contents: "a\n",
        line: "a",
        pattern: Some("b"),
        after: Some("c"),
        indent: true,
        expected: "a\n",
    });

    // 2. If pattern matches, replaces matching line.
    check!(Case {
        contents: "b\n",
        line: "a",
        pattern: Some("b"),
        after: Some("c"),
        indent: true,
        expected: "a\n",
    });

    // 3. If after matches, inserts after matching line.
    check!(Case {
        contents: "c\nd\n",
        line: "a",
        pattern: Some("b"),
        after: Some("c"),
        indent: true,
        expected: "c\na\nd\n",
    });

    // 4. Appends line to end of file.
    check!(Case {
        contents: "d\ne\n",
        line: "a",
        pattern: Some("b"),
        after: Some("c"),
        indent: true,
        expected: "d\ne\na\n",
    });
}

#[test]
fn if_line_exists_does_nothing() {
    // Empty-ish cases.
    check!(Case {
        contents: "",
        line: "",
        pattern: None,
        after: None,
        indent: true,
        expected: "",
    });
    check!(Case {
        contents: "\n",
        line: "",
        pattern: None,
        after: None,
        indent: true,
        expected: "\n",
    });
    check!(Case {
        contents: "   ",
        line: "   ",
        pattern: None,
        after: None,
        indent: true,
        expected: "   ",
    });
    check!(Case {
        contents: "",
        line: "",
        pattern: None,
        after: None,
        indent: false,
        expected: "",
    });
    check!(Case {
        contents: "\n",
        line: "",
        pattern: None,
        after: None,
        indent: false,
        expected: "\n",
    });
    check!(Case {
        contents: "   ",
        line: "   ",
        pattern: None,
        after: None,
        indent: false,
        expected: "   ",
    });

    // With no line ending on the matching line.
    check!(Case {
        contents: "something\nexists",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists",
    });
    check!(Case {
        contents: "something\nexists",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists",
    });

    // With Unix-style line ending.
    check!(Case {
        contents: "something\nexists\n",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\n",
    });
    check!(Case {
        contents: "something\nexists\n",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists\n",
    });

    // With Windows-style line ending.
    check!(Case {
        contents: "something\r\nexists\r\n",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\r\nexists\r\n",
    });
    check!(Case {
        contents: "something\r\nexists\r\n",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\r\nexists\r\n",
    });

    // With Mac-style line ending. Note that this will only work for the last line and
    // absolutely is not a supported usage.
    check!(Case {
        contents: "something\nexists\r",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\r",
    });
    check!(Case {
        contents: "something\nexists\r",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists\r",
    });

    // With trailing white space: in file, on line, and different white space on both.
    check!(Case {
        contents: "something\nexists\t",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\t",
    });
    check!(Case {
        contents: "something\nexists",
        line: "exists  \t",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists",
    });
    check!(Case {
        contents: "something\nexists\t  ",
        line: "exists  ",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\t  ",
    });
    check!(Case {
        contents: "something\nexists\t",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists\t",
    });
    check!(Case {
        contents: "something\nexists",
        line: "exists  \t",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists",
    });
    check!(Case {
        contents: "something\nexists\t  ",
        line: "exists  ",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists\t  ",
    });
}

#[test]
fn indent_line_behavior() {
    // Empty-ish cases.
    check!(Case {
        contents: "",
        line: "   ",
        pattern: None,
        after: None,
        indent: true,
        expected: "",
    });
    check!(Case {
        contents: "\t",
        line: "   ",
        pattern: None,
        after: None,
        indent: true,
        expected: "\t",
    });
    check!(Case {
        contents: " \n\t",
        line: "\t\t\t",
        pattern: None,
        after: None,
        indent: true,
        expected: " \n\t",
    });

    // With leading indentation.
    check!(Case {
        contents: "something\n\t   exists",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\n\t   exists",
    });

    check!(Case {
        contents: "something\n\t   exists",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\n\t   exists\nexists\n",
    });

    // Without leading indentation.
    check!(Case {
        contents: "something\nexists",
        line: "exists",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists",
    });
    check!(Case {
        contents: "something\nexists",
        line: "exists",
        pattern: None,
        after: None,
        indent: false,
        expected: "something\nexists",
    });
}

#[test]
fn if_pattern_matches_replaces_with_line() {
    // Empty-ish cases.
    check!(Case {
        contents: "",
        line: "beep",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: "beep\n",
    });
    check!(Case {
        contents: "\n",
        line: "beep",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: "beep\n",
    });
    check!(Case {
        contents: "x   ",
        line: "beep",
        pattern: Some("   "),
        after: None,
        indent: true,
        expected: "beep\n",
    });

    // Basic cases: substring match, full string match.
    check!(Case {
        contents: "abc",
        line: "beep",
        pattern: Some("bc"),
        after: None,
        indent: true,
        expected: "beep\n",
    });
    check!(Case {
        contents: "abc",
        line: "beep",
        pattern: Some("abc"),
        after: None,
        indent: true,
        expected: "beep\n",
    });

    // Special case: `Some("")` replaces first line.
    check!(Case {
        contents: "line\nsquare\ncube\n",
        line: "beep",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: "beep\nsquare\ncube\n",
    });

    // With no line ending on the matching line.
    check!(Case {
        contents: "something\nexists",
        line: "spooky",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\nspooky\n",
    });

    // With Unix-style line ending.
    check!(Case {
        contents: "something\nexists\n",
        line: "spooky",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\nspooky\n",
    });

    // With Windows-style line ending.
    check!(Case {
        contents: "something\r\nexists\r\n",
        line: "spooky",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\r\nspooky\r\n",
    });

    // With Mac-style line ending. Note that this will only work for the last line and
    // absolutely is not a supported usage.
    check!(Case {
        contents: "something\nexists\r",
        line: "spooky",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\nspooky\r",
    });

    // With trailing white space: in file; matching white space in file and pattern.
    check!(Case {
        contents: "something\nexists\t",
        line: "spooky",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\nspooky\n",
    });
    check!(Case {
        contents: "something\nexists\t",
        line: "spooky",
        pattern: Some("exists\t"),
        after: None,
        indent: true,
        expected: "something\nspooky\n",
    });
}

#[test]
fn pattern_indent_behavior() {
    // Empty-ish cases.
    check!(Case {
        contents: "",
        line: "   curve",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: "curve\n",
    });
    check!(Case {
        contents: "\t",
        line: "   curve",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: "\tcurve\n",
    });
    check!(Case {
        contents: " \n\t",
        line: "\t\t\tcurve",
        pattern: Some(""),
        after: None,
        indent: true,
        expected: " curve\n\t",
    });

    // With leading indentation.
    check!(Case {
        contents: "something\n\t   exists",
        line: "happy",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\n\t   happy\n",
    });
    check!(Case {
        contents: "something\n\t   exists",
        line: "\thappy",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\n\t   happy\n",
    });
    check!(Case {
        contents: "something\n\t   exists",
        line: "\nhappy",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\n\t   happy\n",
    });
    check!(Case {
        contents: "something\n\t   exists",
        line: "happy   ",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\n\t   happy   \n",
    });
    check!(Case {
        contents: "something\n\t   exists  ",
        line: "happy   ",
        pattern: Some("exists"),
        after: None,
        indent: true,
        expected: "something\n\t   happy   \n",
    });

    check!(Case {
        contents: "something\n\t   exists",
        line: "\thappy\t",
        pattern: Some("exists"),
        after: None,
        indent: false,
        expected: "something\n\thappy\t\n",
    });

    // Without leading indentation.
    check!(Case {
        contents: "something\nexists",
        line: "happy",
        pattern: Some("xis"),
        after: None,
        indent: true,
        expected: "something\nhappy\n",
    });
    check!(Case {
        contents: "something\nexists",
        // One could argue that we should strip this trailing white space, but it's not in the
        // spec, so we don't.
        line: "happy ",
        pattern: Some("xis"),
        after: None,
        indent: true,
        expected: "something\nhappy \n",
    });
    check!(Case {
        contents: "something\nexists",
        line: "happy",
        pattern: Some("xis"),
        after: None,
        indent: false,
        expected: "something\nhappy\n",
    });
    check!(Case {
        contents: "something\nexists",
        line: "happy ",
        pattern: Some("xis"),
        after: None,
        indent: false,
        expected: "something\nhappy \n",
    });
}

#[test]
fn pattern_trailing_newline_behavior() {
    // Verify that when a pattern matches the last line, it ensures a trailing newline, and
    // when it matches a line other than the last line, it leaves the last line alone.

    check!(Case {
        contents: "something\nexists",
        line: "beep",
        pattern: Some("exists"),
        after: None,
        indent: false,
        expected: "something\nbeep\n",
    });

    check!(Case {
        contents: "something\nexists\n",
        line: "beep",
        pattern: Some("exists"),
        after: None,
        indent: false,
        expected: "something\nbeep\n",
    });

    check!(Case {
        contents: "something\nexists",
        line: "beep",
        pattern: Some("something"),
        after: None,
        indent: false,
        expected: "beep\nexists",
    });

    check!(Case {
        contents: "something\nexists\n",
        line: "beep",
        pattern: Some("something"),
        after: None,
        indent: false,
        expected: "beep\nexists\n",
    });
}

#[test]
fn if_after_matches_inserts_after_matching_line() {
    // Empty-ish cases, including special case of `Some("")`.
    check!(Case {
        contents: "",
        line: "beep",
        pattern: None,
        after: Some(""),
        indent: true,
        expected: "beep\n",
    });
    check!(Case {
        contents: "\n",
        line: "beep",
        pattern: None,
        after: Some(""),
        indent: true,
        expected: "beep\n",
    });
    check!(Case {
        contents: "\t",
        line: "beep",
        pattern: None,
        after: Some(""),
        indent: true,
        expected: "beep\n\t",
    });
    check!(Case {
        contents: "x   ",
        line: "beep",
        pattern: None,
        after: Some("   "),
        indent: true,
        expected: "x   \nbeep\n",
    });

    // Basic cases: substring match, full string match.
    check!(Case {
        contents: "abc",
        line: "beep",
        pattern: None,
        after: Some("bc"),
        indent: true,
        expected: "abc\nbeep\n",
    });
    check!(Case {
        contents: "abc",
        line: "beep",
        pattern: None,
        after: Some("abc"),
        indent: true,
        expected: "abc\nbeep\n",
    });

    // With no line ending on the matching line. (Must be last line, by construction.)
    check!(Case {
        contents: "something\nexists",
        line: "I think",
        pattern: None,
        after: Some("exists"),
        indent: true,
        expected: "something\nexists\nI think\n",
    });

    // With Unix-style line ending.
    check!(Case {
        contents: "something\nexists\n",
        line: "out there",
        pattern: None,
        after: Some("exists"),
        indent: true,
        expected: "something\nexists\nout there\n",
    });

    // With Windows-style line ending.
    check!(Case {
        contents: "something\r\nexists\r\n",
        line: "in here",
        pattern: None,
        after: Some("exists"),
        indent: true,
        expected: "something\r\nexists\r\nin here\n",
    });

    // With Mac-style line ending. Note that this will only work for the last line and
    // absolutely is not a supported usage.
    //
    // Also note that the outcome specified by this test is the ONLY correct outcome in this
    // case. We CANNOT preserve the Mac-style line ending, otherwise Sira will parse the middle
    // line and the last line as one line on the next run.
    check!(Case {
        contents: "something\nexists\r",
        line: "eerily",
        pattern: None,
        after: Some("exists"),
        indent: true,
        expected: "something\nexists\r\neerily\n",
    });

    // With leading/trailing white space: in file; matching white space in file and `after`.
    check!(Case {
        contents: "\tsomething \t \nexists\n",
        line: "spooky",
        pattern: None,
        after: Some("something"),
        indent: true,
        expected: "\tsomething \t \nspooky\nexists\n",
    });
    check!(Case {
        contents: "\tsomething\t\nexists\n",
        line: "spooky",
        pattern: None,
        after: Some("\tsomething\t"),
        indent: true,
        expected: "\tsomething\t\nspooky\nexists\n",
    });
}

#[test]
fn by_default_appends_line() {
    // Empty-ish cases, including special case of a file containing only white space.
    check!(Case {
        contents: "",
        line: "last",
        pattern: None,
        after: None,
        indent: true,
        expected: "last\n",
    });
    check!(Case {
        contents: "\n",
        line: "last",
        pattern: None,
        after: None,
        indent: true,
        expected: "last\n",
    });
    check!(Case {
        contents: "   ",
        line: "last",
        pattern: None,
        after: None,
        indent: true,
        expected: "last\n",
    });

    // With no line ending on the final line.
    check!(Case {
        contents: "something\nexists",
        line: "all around us",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\nall around us\n",
    });

    // With Unix-style line ending.
    check!(Case {
        contents: "something\nexists\n",
        line: "over our heads",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\nover our heads\n",
    });

    // With Windows-style line ending.
    check!(Case {
        contents: "something\r\nexists\r\n",
        line: "under our feet",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\r\nexists\r\nunder our feet\n",
    });

    // With Mac-style line ending. Note that this will only work for the last line and
    // absolutely is not a supported usage.
    //
    // Also note that the outcome specified by this test is the ONLY correct outcome in this
    // case. We CANNOT preserve the Mac-style line ending, otherwise Sira will parse the middle
    // line and the last line as one line on the next run.
    check!(Case {
        contents: "something\nexists\r",
        line: "hypothetically",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\r\nhypothetically\n",
    });

    // With trailing white space: in file; in line.
    check!(Case {
        contents: "something\nexists\t",
        line: "theoretically",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\t\ntheoretically\n",
    });
    check!(Case {
        contents: "something\nexists",
        line: "actually  \t",
        pattern: None,
        after: None,
        indent: true,
        expected: "something\nexists\nactually  \t\n",
    });
}
