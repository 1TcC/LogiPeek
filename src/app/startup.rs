//! Testable policy for the current-user Windows startup value.

pub const VALUE_NAME: &str = "LogiPeek";

pub trait StartupStore {
    fn read(&mut self, value: &str) -> Result<Option<String>, String>;
    fn write(&mut self, value: &str, data: &str) -> Result<(), String>;
    fn delete(&mut self, value: &str) -> Result<(), String>;
}

pub fn startup_command(exe: &str) -> String {
    format!("{} --startup", quote_windows(exe))
}

pub fn quote_windows(path: &str) -> String {
    let mut quoted = String::from('"');
    let mut backslashes = 0usize;
    for character in path.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '"' {
            quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
        } else {
            quoted.extend(std::iter::repeat_n('\\', backslashes));
        }
        quoted.push(character);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

pub fn is_current(store: &mut impl StartupStore, exe: &str) -> Result<bool, String> {
    Ok(store.read(VALUE_NAME)?.as_deref() == Some(startup_command(exe).as_str()))
}

pub fn enable(store: &mut impl StartupStore, exe: &str) -> Result<(), String> {
    let command = startup_command(exe);
    if store.read(VALUE_NAME)?.as_deref() != Some(command.as_str()) {
        store.write(VALUE_NAME, &command)?;
    }
    Ok(())
}

pub fn disable(store: &mut impl StartupStore) -> Result<(), String> {
    store.delete(VALUE_NAME)
}
