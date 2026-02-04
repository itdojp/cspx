use crate::types::Diagnostic;

pub struct FrontendOutput<IR> {
    pub ir: IR,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait Frontend {
    type Ir;
    type Error;

    fn parse_and_typecheck(
        &self,
        input: &str,
        path: &str,
    ) -> Result<FrontendOutput<Self::Ir>, Self::Error>;
}
