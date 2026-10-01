use std::path::{Path, PathBuf};

use crate::errors::IrrecoverableError;

#[derive(Debug, Clone)]
pub struct PathResolver {
    // Necessary for resolving absolute DWARF paths to relative paths.
    compile_dir: PathBuf,
    // Necessary for resolving relative paths to current runtime paths.
    runtime_dir: PathBuf,
}

impl PathResolver {
    pub fn new(compile_dir: PathBuf, runtime_dir: PathBuf) -> Self {
        Self {
            compile_dir,
            runtime_dir,
        }
    }

    pub fn relative_path_to_runtime_path(&self, relative_path: &Path) -> PathBuf {
        self.runtime_dir.join(relative_path)
    }

    pub fn dwarf_path_to_relative_path(
        &self,
        dwarf_path: &Path,
    ) -> Result<PathBuf, IrrecoverableError> {
        dwarf_path
            .strip_prefix(&self.compile_dir)
            .map(|path| path.to_path_buf())
            .map_err(|_| IrrecoverableError::DwarfPath {
                path: dwarf_path.display().to_string(),
                compile_dir: self.compile_dir.display().to_string(),
            })
    }

    pub fn dwarf_path_to_runtime_path(
        &self,
        dwarf_path: &Path,
    ) -> Result<PathBuf, IrrecoverableError> {
        Ok(self.relative_path_to_runtime_path(&self.dwarf_path_to_relative_path(dwarf_path)?))
    }

    pub fn runtime_path_to_relative_path(&self, runtime_path: &Path) -> PathBuf {
        runtime_path
            .strip_prefix(&self.runtime_dir)
            .expect("Runtime path does not correspond to runtime directory")
            .to_path_buf()
    }

    pub fn compile_dir(&self) -> &Path {
        &self.compile_dir
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;

    use crate::errors::IrrecoverableError;
    use crate::sources::Sources;

    use super::PathResolver;

    #[test]
    fn path_outside_compile_dir_is_dwarf_path() {
        let resolver = PathResolver::new("/compile".into(), "/runtime".into());
        let err = resolver
            .dwarf_path_to_relative_path(Path::new("/other/file.rs"))
            .unwrap_err();
        assert!(matches!(
            err,
            IrrecoverableError::DwarfPath { ref path, ref compile_dir }
                if path.ends_with("file.rs") && compile_dir.ends_with("compile")
        ));
        let sources = Sources::new(resolver, HashSet::new());
        assert!(!sources.is_valid_source(Path::new("/other/file.rs"), 1));
    }
}
