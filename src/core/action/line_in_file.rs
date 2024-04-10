use super::Action;
use std::borrow::Cow;
use std::fs;
use std::io;

// TODO Consider rewriting LineInFile to use regex.
//
// Reasons to do this:
// - Flexibility / robustness in messy user environments
// - Obvious correctness
//
// Reasons not to do this:
// - Requires the Regex crate (although we currently require it elsewhere, too, so it's fine)
// - Might be a significant performance regression, particularly if the regex needs to be compiled
//   each run. (Try to mitigate this by ensuring that it's computed at compile time as a
//   compile-time-constant value.)

/// Implements client-side logic for [Action::LineInFile].
///
/// # Returns
///
/// Returns `Ok(())` on success, regardless of whether the file was modified. Returns an error if
/// the file cannot be read or written.
///
/// # Panics
///
/// Panics if `action` is not of type [Action::LineInFile].
pub fn line_in_file(action: &Action) -> io::Result<()> {
    let (path, line, pattern, after, &indent) = match action {
        Action::LineInFile {
            path,
            line,
            pattern,
            after,
            indent,
        } => (path, line, pattern, after, indent),
        _ => panic!("called _line_in_file with an Action that was not a LineInFile: {action:?}"),
    };

    let mut file = fs::read_to_string(path)?;

    if line_is_present(&file, line, indent) {
        return Ok(());
    }

    if let Some(pattern) = pattern {
        if replace_pattern(&mut file, line, pattern, indent) {
            fs::write(path, &file)?;
            return Ok(());
        }
    }

    if let Some(after) = after {
        if insert_after(&mut file, line, after) {
            fs::write(path, &file)?;
            return Ok(());
        }
    }

    append_line(&mut file, line);
    fs::write(path, &file)?;
    Ok(())
}

/// Returns whether `line` is present in `file`.
fn line_is_present(file: &str, mut line: &str, indent: bool) -> bool {
    if indent {
        line = line.trim_start();
    }
    line = line.trim_end();

    for mut file_line in file.split('\n') {
        if indent {
            file_line = file_line.trim_start();
        }
        file_line = file_line.trim_end();

        if file_line == line {
            return true;
        }
    }
    false
}

/// If `pattern` matches a line in `file`, replaces the line with `line`. Returns whether a match
/// occurred.
fn replace_pattern(file: &mut String, mut line: &str, pattern: &str, indent: bool) -> bool {
    // When we insert `line` into `file`, we need to ensure that it is terminated by a '\n'
    // character. This function checks whether `byte_index` into `file` contains '\n' and returns
    // either `line` or `line` plus '\n' as appropriate.
    fn format_line<'a>(file: &str, byte_index: usize, line: &'a str) -> Cow<'a, str> {
        if let Some(s) = file.get(byte_index..) {
            if let Some('\n') = s.chars().next() {
                return Cow::Borrowed(line);
            }
        }
        Cow::Owned(format!("{line}\n"))
    }

    for mut file_line in file.split('\n') {
        if file_line.contains(pattern) {
            if indent {
                file_line = file_line.trim_start();
                line = line.trim_start();
            }

            // Use safe pointer math to efficiently calculate the starting index of `file_line`
            // within `file`.
            let file_line_start = (file_line.as_ptr() as usize)
                .checked_sub(file.as_ptr() as usize)
                .expect("BUG: subtracting file_line from file should never underflow");

            // Edge case: if `file_line.is_empty()`, then we simply need to insert `line`.
            if file_line.is_empty() {
                let formatted = format_line(file, file_line_start, line);
                file.insert_str(file_line_start, &formatted);
                return true;
            }

            // The index of the first byte after `file_line` in `file`. If `file_line` is at the
            // end of `file`, then this will be one past the end of `file`.
            let mut file_line_end = file_line_start + file_line.len();

            // Replace the remaining portion of `file_line` within `file` with `line`. Add '\n' to
            // the end of `line` if need be.
            let replacement: Cow<str> = if let Some('\r') = file_line.chars().next_back() {
                // `file_line` ends with '\r', so we will forego adding a trailing '\n' , and we
                // will preserve the '\r' character.
                //
                // The reason we don't simply `trim_end()` is that, in accordance with basic text
                // file etiquette, if there's trailing white space other than an expected line
                // terminator, that's an error, so we should remove it. We're about to replace
                // `file_line`, so keeping the erroneous white space in `file_line` causes it to be
                // replaced when we call String::replace_range.

                // `file_line_end >= file_line.len() > 0`, therefore we can safely decrement.
                file_line_end -= 1;
                line.into()
            } else {
                format_line(file, file_line_end, line)
            };
            file.replace_range(file_line_start..file_line_end, &replacement);
            return true;
        }
    }
    false
}

/// If `after` matches a line in `file`, inserts `line` right after the matching line. Returns
/// whether a match occurred.
fn insert_after(file: &mut String, line: &str, after: &str) -> bool {
    // Special case from the specification: if `after == ""`, insert at start of file.
    if after.is_empty() {
        if file.starts_with('\n') {
            file.insert_str(0, line);
        } else {
            file.insert_str(0, &format!("{line}\n"));
        }
        return true;
    }

    for file_line in file.split('\n') {
        if file_line.contains(after) {
            // Use safe pointer math to efficiently calculate the index of the first byte after
            // `file_line` in `file`. If `file_line` is at the end of `file`, then this will be one
            // past the end of `file`.
            //
            // For maximum safety from overflows and underflows, we use checked arithmetic, and we
            // subtract before adding. This should provably prevent overflow and underflow.
            let insert_index = (file_line.as_ptr() as usize)
                .checked_sub(file.as_ptr() as usize)
                .expect("BUG: subtracting file_line from file should never underflow")
                .checked_add(file_line.len())
                .expect("BUG: adding file_line.len() to insert_index should never overflow");

            // We split on '\n', so insert_index is either `file.len()` or the index of '\n'
            // character. In the former case, the file does not end in '\n' and we are modifying
            // the last line, so we'll add a trailing newline.
            if insert_index == file.len() {
                file.push('\n');
                file.push_str(line);
                file.push('\n');
            } else {
                // We're inserting before the newline, so the newline we insert goes at the start
                // of the inserted string to terminate `file_line`.
                file.insert_str(insert_index, &format!("\n{line}"));
            }
            return true;
        }
    }
    false
}

/// Adds `line` as a new line at the end of `file`.
fn append_line(file: &mut String, line: &str) {
    if file.trim_start().is_empty() && !line.trim_start().is_empty() {
        file.clear();
    } else if !file.ends_with('\n') && !file.is_empty() {
        file.push('\n');
    }
    file.push_str(line);
    file.push('\n');
}

#[cfg(test)]
mod test;
