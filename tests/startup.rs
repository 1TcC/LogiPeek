use logipeek::app::startup::{self, StartupStore, disable, enable, is_current};

#[derive(Default)]
struct Mock {
    value: Option<String>,
    fail: bool,
    writes: usize,
}
impl StartupStore for Mock {
    fn read(&mut self, _: &str) -> Result<Option<String>, String> {
        if self.fail {
            Err("read".into())
        } else {
            Ok(self.value.clone())
        }
    }
    fn write(&mut self, _: &str, data: &str) -> Result<(), String> {
        if self.fail {
            Err("write".into())
        } else {
            self.value = Some(data.into());
            self.writes += 1;
            Ok(())
        }
    }
    fn delete(&mut self, _: &str) -> Result<(), String> {
        if self.fail {
            Err("delete".into())
        } else {
            self.value = None;
            Ok(())
        }
    }
}

#[test]
fn quotes_spaces_and_startup_switch() {
    assert_eq!(
        startup::startup_command(r#"C:\Program Files\LogiPeek\logipeek.exe"#),
        r#""C:\Program Files\LogiPeek\logipeek.exe" --startup"#
    );
}
#[test]
fn enable_read_disable() {
    let mut m = Mock::default();
    enable(&mut m, "x.exe").unwrap();
    enable(&mut m, "x.exe").unwrap();
    assert_eq!(m.writes, 1);
    assert!(is_current(&mut m, "x.exe").unwrap());
    disable(&mut m).unwrap();
    assert!(!is_current(&mut m, "x.exe").unwrap());
}
#[test]
fn errors_propagate() {
    let mut m = Mock {
        fail: true,
        ..Default::default()
    };
    assert!(enable(&mut m, "x").is_err());
    assert!(is_current(&mut m, "x").is_err());
    assert!(disable(&mut m).is_err());
}

#[test]
fn enable_repairs_a_moved_executable_path() {
    let mut mock = Mock {
        value: Some("\"C:\\Old\\logipeek.exe\" --startup".into()),
        ..Mock::default()
    };
    enable(&mut mock, r"C:\New Folder\logipeek.exe").unwrap();
    assert_eq!(mock.writes, 1);
    assert!(is_current(&mut mock, r"C:\New Folder\logipeek.exe").unwrap());
}
