use std::{
    fs::OpenOptions,
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo,
        GetFileInformationByHandleEx,
    },
    System::SystemServices::IO_REPARSE_TAG_AF_UNIX,
};

/// Positively classify an occupied endpoint without following it or reading
/// content. A connection error alone never authorizes removing a filesystem path.
pub fn is_unix_socket(path: &Path) -> io::Result<bool> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut information = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    // SAFETY: file keeps a valid owned handle alive for this synchronous call.
    // The non-following handle names the entry itself. FileAttributeTagInfo
    // requires exactly this initialized output structure and its byte size.
    // The API retains neither pointer and returns nonzero only on success.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut information as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of_val(&information) as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(
        information.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            && information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            && information.ReparseTag == IO_REPARSE_TAG_AF_UNIX,
    )
}
