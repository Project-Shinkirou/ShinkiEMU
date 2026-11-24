use crate::{GameAssets, NativeCall, Step, Vm, Word};
use shinki_emu_lucifen_formats::member_key;
use std::{io, path::Path};

const SCRIPT_LIMIT: usize = 16 * 1024 * 1024;

enum Transition {
    Call(String),
    Replace(String),
    End,
}

pub struct Game {
    assets: GameAssets,
    vm: Vm,
    transition: Option<Transition>,
}

impl Game {
    pub fn open(root: &Path) -> io::Result<Self> {
        let mut assets = GameAssets::open(root)?;
        // MAIN and FUNC_LIB are the standard native Lucifen launch modules.
        let main = assets.read("script/main.sob", SCRIPT_LIMIT)?;
        let library = match assets.read("script/func_lib.sob", SCRIPT_LIMIT) {
            Ok(library) => Some(library),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let mut vm = Vm::open(&main, library.as_deref())?;
        match assets.read("script/user_var.sob", SCRIPT_LIMIT) {
            Ok(variables) => vm.set_user_variables(&variables)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(Self {
            assets,
            vm,
            transition: None,
        })
    }

    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
    }

    pub fn assets_mut(&mut self) -> &mut GameAssets {
        &mut self.assets
    }

    /// Service the VM kernel's own script-transition calls. Platform operations
    /// yield a native call to the caller and wait for its explicit result.
    pub fn advance(&mut self, budget: usize) -> io::Result<Step> {
        let step = self.vm.advance(budget)?;
        match step {
            Step::Calling(call) if call.function <= 2 => {
                self.transition = Some(match call.function {
                    0 => Transition::Call(self.script_argument(call)?),
                    1 => Transition::Replace(self.script_argument(call)?),
                    _ => Transition::End,
                });
                self.vm.complete_call(Word::Integer(0))?;
                Ok(Step::BudgetExhausted)
            }
            Step::Yield
                if matches!(self.transition, Some(Transition::Call(_) | Transition::End)) =>
            {
                match self.transition.take().unwrap() {
                    Transition::Call(name) => {
                        let bytes = self.assets.read(&name, SCRIPT_LIMIT)?;
                        self.vm.enter_script(&bytes, true)?;
                        Ok(Step::BudgetExhausted)
                    }
                    Transition::End => {
                        self.vm.finish();
                        Ok(Step::Finished)
                    }
                    _ => unreachable!("matched Lucifen yield transition"),
                }
            }
            Step::Finished => match self.transition.take() {
                Some(Transition::Replace(name)) => {
                    let bytes = self.assets.read(&name, SCRIPT_LIMIT)?;
                    self.vm.enter_script(&bytes, false)?;
                    Ok(Step::BudgetExhausted)
                }
                Some(Transition::Call(_)) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Lucifen script returned before making a nested call",
                )),
                _ if self.vm.resume_caller() => Ok(Step::BudgetExhausted),
                _ => Ok(Step::Finished),
            },
            step => Ok(step),
        }
    }

    fn script_argument(&self, call: NativeCall) -> io::Result<String> {
        let address = self.vm.argument(call, 0)?.address()?;
        let bytes = self.vm.text(address)?;
        let name = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&bytes)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen script name")
            })?;
        let key = member_key(&name)?;
        if key.contains('/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Lucifen script basename",
            ));
        }
        Ok(format!("script/{key}.sob"))
    }
}
