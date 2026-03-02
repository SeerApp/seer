use crate::contexts::seer::SeerContext;

pub struct SourcesContext {
    seer: SeerContext,
    // eps: Option<ExternalProgramService>,
}

impl SourcesContext {
    pub async fn new() -> Self {
        // let eps = match Config::from_env().ok().expect("Error in parsing config") {
        //     Some(config) => Some(ExternalProgramService::new(config).await),
        //     None => {
        //         None
        //     }
        // };

        Self {
            seer: SeerContext::new(),
            // eps,
        }
    }

    pub fn get_context(&mut self) -> &mut SeerContext {
        &mut self.seer
    }

    // pub async fn get_account(&mut self, pubkey: &Pubkey) -> Option<Vec<u8>> {
    //     if let Some(eps) = self.eps.as_mut() {
    //         if let Ok(Some((elf, tmp_dir, dwarf_compile_path))) = eps.get_account(&pubkey).await {
    //             self.seer.add_lookups(
    //                 &tmp_dir.path().to_path_buf(),
    //                 &PathBuf::from(dwarf_compile_path.as_str()),
    //             );
    //             return Some(elf);
    //         }
    //     }

    //     None
    // }
}