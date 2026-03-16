use std::path::PathBuf;

#[derive(Debug)]
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

    pub fn relative_path_to_runtime_path(&self, relative_path: &PathBuf) -> PathBuf {
        self.runtime_dir.join(relative_path)
    }

    pub fn dwarf_path_to_relative_path(&self, dwarf_path: &PathBuf) -> anyhow::Result<PathBuf> {
        Ok(dwarf_path
            .strip_prefix(self.compile_dir.clone())
            .map_err(|e| anyhow::anyhow!(
                "DWARF path {:?} does not correspond to compile directory {:?}: {}",
                dwarf_path,
                self.compile_dir,
                e,
            ))?
            .to_path_buf())
    }

    pub fn dwarf_path_to_runtime_path(&self, dwarf_path: &PathBuf) -> anyhow::Result<PathBuf> {
        Ok(self.relative_path_to_runtime_path(&self.dwarf_path_to_relative_path(dwarf_path)?))
    }

    pub fn runtime_path_to_relative_path(&self, runtime_path: &PathBuf) -> PathBuf {
        runtime_path
            .strip_prefix(self.runtime_dir.clone())
            .expect("Runtime path does not correspond to runtime directory")
            .to_path_buf()
    }

    pub fn compile_dir(&self) -> &PathBuf {
        &self.compile_dir
    }

    pub fn runtime_dir(&self) -> &PathBuf {
        &self.runtime_dir
    }
}
