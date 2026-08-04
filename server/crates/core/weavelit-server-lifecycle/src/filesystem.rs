use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};

use rustix::{
    fs::{self, AtFlags, Dir, FileType, Mode, OFlags, RawMode, RenameFlags},
    io::Errno,
    process,
};

use crate::{
    LifecycleError, LocatorGeneration,
    format::{
        KEY_FILE_NAME, LOCK_FILE_NAME, RECORD_FILE_NAME, parse_generation_token,
        parse_locator_file_name, temporary_file_name,
    },
};

const ROOT_MODE: RawMode = 0o700;
const FILE_MODE: RawMode = 0o600;
const MAX_ROOT_ENTRIES: usize = 256;
const APPLICATION_DATABASE_SQLITE_FILES: &[&str] = &[
    "application.sqlite3",
    "application.sqlite3-journal",
    "application.sqlite3-wal",
    "application.sqlite3-shm",
];
const LOG_DATABASE_SQLITE_FILES: &[&str] = &[
    "log.sqlite3",
    "log.sqlite3-journal",
    "log.sqlite3-wal",
    "log.sqlite3-shm",
];

#[derive(Debug)]
pub(crate) struct Inventory {
    has_lock: bool,
    pub(crate) has_key: bool,
    pub(crate) has_record: bool,
    pub(crate) locator_files: Vec<(LocatorGeneration, String)>,
    pub(crate) temporary_files: Vec<String>,
    pub(crate) has_application_database_artifact: bool,
    pub(crate) has_log_database_artifact: bool,
}

pub(crate) struct StateRoot {
    directory: File,
    _lock: File,
}

impl StateRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, LifecycleError> {
        if !path.is_absolute() || process::geteuid().is_root() {
            return Err(LifecycleError::ConfigurationInvalid);
        }

        process::umask(Mode::from_raw_mode(0o077));
        let mut directory = fs::open(
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| LifecycleError::ConfigurationInvalid)?;
        let mut saw_root = false;
        for component in path.components() {
            match component {
                Component::RootDir if !saw_root => saw_root = true,
                Component::Normal(name) if saw_root => {
                    directory = fs::openat(
                        &directory,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                        Mode::empty(),
                    )
                    .map_err(|_| LifecycleError::ConfigurationInvalid)?;
                }
                _ => return Err(LifecycleError::ConfigurationInvalid),
            }
        }
        if !saw_root {
            return Err(LifecycleError::ConfigurationInvalid);
        }
        validate_root(&directory)?;
        let directory = File::from(directory);
        let (lock, lock_was_created) = open_lock(&directory)?;
        let inventory = inspect_inventory(&directory)?;
        if !inventory.has_lock
            || (lock_was_created && !inventory.is_empty())
            || (!lock_was_created && inventory.is_empty())
        {
            return Err(LifecycleError::IntegrityFailure);
        }
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub(crate) fn inventory(&self) -> Result<Inventory, LifecycleError> {
        inspect_inventory(&self.directory)
    }
}

impl Inventory {
    fn is_empty(&self) -> bool {
        !self.has_key
            && !self.has_record
            && self.locator_files.is_empty()
            && self.temporary_files.is_empty()
            && !self.has_application_database_artifact
            && !self.has_log_database_artifact
    }
}

fn inspect_inventory(directory: &File) -> Result<Inventory, LifecycleError> {
    let iterator_fd = fs::openat(
        directory,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| LifecycleError::Persistence)?;
    let mut iterator = Dir::new(iterator_fd).map_err(|_| LifecycleError::Persistence)?;
    let mut names = Vec::new();
    while let Some(entry) = iterator.read() {
        let entry = entry.map_err(|_| LifecycleError::IntegrityFailure)?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        if names.len() == MAX_ROOT_ENTRIES {
            return Err(LifecycleError::IntegrityFailure);
        }
        let name = std::str::from_utf8(name)
            .map_err(|_| LifecycleError::IntegrityFailure)?
            .to_owned();
        validate_child_name_and_metadata(directory, &name)?;
        names.push(name);
    }

    let mut inventory = Inventory {
        has_lock: false,
        has_key: false,
        has_record: false,
        locator_files: Vec::new(),
        temporary_files: Vec::new(),
        has_application_database_artifact: false,
        has_log_database_artifact: false,
    };
    for name in names {
        match name.as_str() {
            LOCK_FILE_NAME => inventory.has_lock = true,
            KEY_FILE_NAME => inventory.has_key = true,
            RECORD_FILE_NAME => inventory.has_record = true,
            name if APPLICATION_DATABASE_SQLITE_FILES.contains(&name) => {
                inventory.has_application_database_artifact = true;
            }
            name if LOG_DATABASE_SQLITE_FILES.contains(&name) => {
                inventory.has_log_database_artifact = true;
            }
            _ => {
                if let Some(generation) = parse_locator_file_name(&name)? {
                    inventory.locator_files.push((generation, name));
                } else if is_valid_temporary_name(&name)? {
                    inventory.temporary_files.push(name);
                } else {
                    return Err(LifecycleError::IntegrityFailure);
                }
            }
        }
    }
    inventory
        .locator_files
        .sort_by(|left, right| left.1.cmp(&right.1));
    inventory.temporary_files.sort();
    Ok(inventory)
}

impl StateRoot {
    pub(crate) fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, LifecycleError> {
        let descriptor = fs::openat(
            &self.directory,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| LifecycleError::IntegrityFailure)?;
        validate_file(&descriptor)?;
        let file = File::from(descriptor);
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| LifecycleError::Persistence)?;
        if bytes.len() > limit {
            return Err(LifecycleError::IntegrityFailure);
        }
        Ok(bytes)
    }

    pub(crate) fn publish_new(&self, name: &str, bytes: &[u8]) -> Result<(), LifecycleError> {
        self.write_atomic(name, bytes, false, &NoFault)
    }

    pub(crate) fn replace(&self, name: &str, bytes: &[u8]) -> Result<(), LifecycleError> {
        self.write_atomic(name, bytes, true, &NoFault)
    }

    pub(crate) fn remove(&self, name: &str) -> Result<(), LifecycleError> {
        fs::unlinkat(&self.directory, name, AtFlags::empty())
            .map_err(|_| LifecycleError::Persistence)?;
        self.sync_directory()
    }

    pub(crate) fn sync_directory(&self) -> Result<(), LifecycleError> {
        self.directory
            .sync_all()
            .map_err(|_| LifecycleError::Persistence)
    }

    fn write_atomic<O: WriteObserver>(
        &self,
        name: &str,
        bytes: &[u8],
        replace: bool,
        observer: &O,
    ) -> Result<(), LifecycleError> {
        let temporary = temporary_file_name(name)?;
        let result = (|| {
            let descriptor = fs::openat(
                &self.directory,
                temporary.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::from_raw_mode(FILE_MODE),
            )
            .map_err(|_| LifecycleError::Persistence)?;
            validate_file(&descriptor)?;
            let mut file = File::from(descriptor);
            observer.before(WriteStage::Write)?;
            file.write_all(bytes)
                .map_err(|_| LifecycleError::Persistence)?;
            observer.before(WriteStage::FileSync)?;
            sync_file(&file)?;
            observer.before(WriteStage::Rename)?;
            if replace {
                fs::renameat(&self.directory, temporary.as_str(), &self.directory, name)
            } else {
                fs::renameat_with(
                    &self.directory,
                    temporary.as_str(),
                    &self.directory,
                    name,
                    RenameFlags::NOREPLACE,
                )
            }
            .map_err(|_| LifecycleError::Persistence)?;
            observer.before(WriteStage::DirectorySync)?;
            self.sync_directory()
        })();
        if result.is_err() {
            let _ = fs::unlinkat(&self.directory, temporary.as_str(), AtFlags::empty());
        }
        result
    }
}

impl fmt::Debug for StateRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StateRoot(REDACTED)")
    }
}

fn open_lock(directory: &File) -> Result<(File, bool), LifecycleError> {
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let (descriptor, was_created) = match fs::openat(
        directory,
        LOCK_FILE_NAME,
        flags | OFlags::EXCL,
        Mode::from_raw_mode(FILE_MODE),
    ) {
        Ok(descriptor) => (descriptor, true),
        Err(Errno::EXIST) => (
            fs::openat(
                directory,
                LOCK_FILE_NAME,
                flags,
                Mode::from_raw_mode(FILE_MODE),
            )
            .map_err(|_| LifecycleError::ConfigurationInvalid)?,
            false,
        ),
        Err(_) => return Err(LifecycleError::ConfigurationInvalid),
    };
    validate_file(&descriptor)?;
    let lock = File::from(descriptor);
    lock.try_lock().map_err(|_| LifecycleError::LockContended)?;
    Ok((lock, was_created))
}

fn validate_root<Fd: std::os::fd::AsFd>(descriptor: &Fd) -> Result<(), LifecycleError> {
    let metadata = fs::fstat(descriptor).map_err(|_| LifecycleError::ConfigurationInvalid)?;
    let effective_user = process::geteuid();
    if !FileType::from_raw_mode(metadata.st_mode).is_dir()
        || metadata.st_uid != effective_user.as_raw()
        || metadata.st_mode & 0o777 != ROOT_MODE
    {
        return Err(LifecycleError::ConfigurationInvalid);
    }
    Ok(())
}

fn validate_file<Fd: std::os::fd::AsFd>(descriptor: &Fd) -> Result<(), LifecycleError> {
    let metadata = fs::fstat(descriptor).map_err(|_| LifecycleError::IntegrityFailure)?;
    if !FileType::from_raw_mode(metadata.st_mode).is_file()
        || metadata.st_uid != process::geteuid().as_raw()
        || metadata.st_mode & 0o777 != FILE_MODE
        || metadata.st_nlink != 1
    {
        return Err(LifecycleError::ConfigurationInvalid);
    }
    Ok(())
}

fn validate_child_name_and_metadata(directory: &File, name: &str) -> Result<(), LifecycleError> {
    let metadata = fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| LifecycleError::IntegrityFailure)?;
    if !FileType::from_raw_mode(metadata.st_mode).is_file()
        || metadata.st_uid != process::geteuid().as_raw()
        || metadata.st_mode & 0o777 != FILE_MODE
        || metadata.st_nlink != 1
    {
        return Err(LifecycleError::ConfigurationInvalid);
    }
    Ok(())
}

fn is_valid_temporary_name(name: &str) -> Result<bool, LifecycleError> {
    for prefix in [
        format!("{KEY_FILE_NAME}.tmp-"),
        format!("{RECORD_FILE_NAME}.tmp-"),
    ] {
        if let Some(token) = name.strip_prefix(&prefix) {
            parse_generation_token(token)?;
            return Ok(true);
        }
    }
    if let Some((locator, token)) = name.rsplit_once(".tmp-")
        && parse_locator_file_name(locator)?.is_some()
    {
        parse_generation_token(token)?;
        return Ok(true);
    }
    Ok(false)
}

fn sync_file(file: &File) -> Result<(), LifecycleError> {
    file.sync_all().map_err(|_| LifecycleError::Persistence)?;
    #[cfg(target_vendor = "apple")]
    rustix::fs::fcntl_fullfsync(file).map_err(|_| LifecycleError::Persistence)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteStage {
    Write,
    FileSync,
    Rename,
    DirectorySync,
}

trait WriteObserver {
    fn before(&self, stage: WriteStage) -> Result<(), LifecycleError>;
}

struct NoFault;

impl WriteObserver for NoFault {
    fn before(&self, _stage: WriteStage) -> Result<(), LifecycleError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    struct FaultAt(WriteStage);

    impl WriteObserver for FaultAt {
        fn before(&self, stage: WriteStage) -> Result<(), LifecycleError> {
            if stage == self.0 {
                Err(LifecycleError::Persistence)
            } else {
                Ok(())
            }
        }
    }

    fn root() -> (tempfile::TempDir, StateRoot) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let canonical_path = directory.path().canonicalize().unwrap();
        let root = StateRoot::open(&canonical_path).unwrap();
        (directory, root)
    }

    #[test]
    fn injected_write_failures_never_publish_partial_bytes() {
        let (directory, root) = root();
        root.publish_new(RECORD_FILE_NAME, b"old").unwrap();

        for stage in [
            WriteStage::Write,
            WriteStage::FileSync,
            WriteStage::Rename,
            WriteStage::DirectorySync,
        ] {
            std::fs::write(directory.path().join(RECORD_FILE_NAME), b"old").unwrap();
            let result = root.write_atomic(RECORD_FILE_NAME, b"new", true, &FaultAt(stage));
            assert_eq!(result.unwrap_err(), LifecycleError::Persistence);
            let bytes = std::fs::read(directory.path().join(RECORD_FILE_NAME)).unwrap();
            if stage == WriteStage::DirectorySync {
                assert_eq!(bytes, b"new");
            } else {
                assert_eq!(bytes, b"old");
            }
            assert!(root.inventory().unwrap().temporary_files.is_empty());
        }
    }
}
