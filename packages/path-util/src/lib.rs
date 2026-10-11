use itertools::Itertools;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::value::StringDeserializer,
};
use typed_path::{
    Utf8Component, Utf8TypedPathBuf, Utf8UnixComponent, Utf8UnixPathBuf,
};

#[derive(
    Eq, PartialEq, Hash, Debug, Clone, derive_more::Display, derive_more::Deref,
)]
#[repr(transparent)]
pub struct SafeRelativeUtf8UnixPathBuf(Utf8UnixPathBuf);

pub fn is_safe_file_name(file_name: &str) -> bool {
    if file_name.contains('/') || file_name.contains('\\') {
        return false;
    }

    SafeRelativeUtf8UnixPathBuf::try_from(file_name.to_string()).is_ok_and(
        |path| {
            path.components()
                .exactly_one()
                .is_ok_and(|component| component.is_normal())
        },
    )
}

impl<'de> Deserialize<'de> for SafeRelativeUtf8UnixPathBuf {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        // When parsed successfully, the path is guaranteed to be free from leading backslashes
        // and Windows prefixes (e.g., `C:`)
        let Utf8TypedPathBuf::Unix(path) =
            Utf8TypedPathBuf::from(String::deserialize(deserializer)?)
        else {
            return Err(serde::de::Error::custom(
                "File path must be a Unix-style relative path",
            ));
        };

        let mut path_components = path.components().peekable();

        if path_components.peek().is_none() {
            return Err(serde::de::Error::custom("File path cannot be empty"));
        }

        // All components should be normal: a file or directory name, not `/`, or `..`,
        // and not refer to any reserved Windows device name. Also, at this point we may have
        // a pseudo-Unix path like `my\directory`, which we should reject by filtering out
        // backslashes to guarantee consistent cross-platform behavior when interpreting component
        // separators
        if !path_components.all(|component| {
            (component.is_normal() || component.is_current())
                && !component.as_str().contains('\\')
                && !is_reserved_windows_device_name(&component)
        }) {
            return Err(serde::de::Error::custom(
                "File path cannot contain any special component, prefix, reserved Windows device name, or backslashes",
            ));
        }

        Ok(Self(path))
    }
}

impl Serialize for SafeRelativeUtf8UnixPathBuf {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut path_components = self.0.components().peekable();

        if path_components.peek().is_none() {
            return Err(serde::ser::Error::custom("File path cannot be empty"));
        }

        if !path_components.all(|component| {
            (component.is_normal() || component.is_current())
                && !component.as_str().contains('\\')
                && !is_reserved_windows_device_name(&component)
        }) {
            return Err(serde::ser::Error::custom(
                "File path cannot contain any special component, prefix, reserved Windows device name, or backslashes",
            ));
        }

        // Iterating over components does basic normalization by e.g. removing redundant
        // slashes and collapsing `.` components, so do that to produce a cleaner output
        // friendlier to the strict deserialization algorithm above
        self.0.components().join("/").serialize(serializer)
    }
}

impl TryFrom<String> for SafeRelativeUtf8UnixPathBuf {
    type Error = serde::de::value::Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::deserialize(StringDeserializer::new(s))
    }
}

/// Whether a bare file or directory name is a reserved Windows device name
/// (`CON`, `NUL`, `COM1`, …, optionally with an extension or ADS suffix).
///
/// Exposed so id validators that use a name as a folder name can reject the
/// same set; on Windows such a folder cannot be created at all.
pub fn is_reserved_windows_device_name_str(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    RESERVED_WINDOWS_DEVICE_NAMES.iter().any(|reserved| {
        upper.starts_with(reserved)
            && (upper[reserved.len()..].is_empty()
                || upper[reserved.len()..].starts_with('.')
                || upper[reserved.len()..].starts_with(':'))
    })
}

const RESERVED_WINDOWS_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5",
    "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²",
    "LPT³", "CONIN$", "CONOUT$",
];

fn is_reserved_windows_device_name(component: &Utf8UnixComponent) -> bool {
    is_reserved_windows_device_name_str(component.as_str())
}

#[test]
fn safe_relative_path_deserialization_contract() {
    let valid_paths = [
        "file.txt",
        "directory/file.txt",
        "my-directory/file.name.with.dots.tar.gz",
        "my_directory/123_456-789.file",
        "./my/file.txt",
        "my/./file.txt",
    ];
    for path in valid_paths {
        SafeRelativeUtf8UnixPathBuf::try_from(path.to_string())
            .expect("Path should be considered valid");
    }

    let invalid_paths = [
        "",                        // Empty path
        "/absolute/file.txt",      // Absolute path
        "C:/absolute/file.txt",    // Absolute path with common Windows prefix
        "//server/share/file.txt", // Absolute path with Windows UNC prefix
        "directory/../file.txt",   // Path with `..` component
        "CON.txt",                 // Reserved Windows device name
        "NUL/file.txt",            // Reserved Windows device name "directory"
        "COM1.txt:ads",            // Reserved Windows device name with ADS name
        "file\\name.txt",          // Backslash in file name
        "my\\directory/file.txt",  // Backslash in directory name
    ];
    for path in invalid_paths {
        SafeRelativeUtf8UnixPathBuf::try_from(path.to_string())
            .expect_err("Path should be considered invalid");
    }
}

#[test]
fn safe_file_name_contract() {
    let valid_file_names = [
        "file.txt",
        "file.name.with.dots.tar.gz",
        "file..name.jar",
        "123_456-789.file",
    ];
    for file_name in valid_file_names {
        assert!(is_safe_file_name(file_name));
    }

    let invalid_file_names = [
        "",
        ".",
        "..",
        "../file.txt",
        "directory/file.txt",
        r"..\file.txt",
        r"directory\file.txt",
        "C:file.txt",
        "C:/file.txt",
        "NUL.txt",
    ];
    for file_name in invalid_file_names {
        assert!(!is_safe_file_name(file_name));
    }
}
