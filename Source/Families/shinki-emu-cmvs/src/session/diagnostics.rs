use crate::CmvsPs2aInstructionFrame;
use shinki_emu_sdk::runtime::SessionError;

pub(super) fn instruction(
    frame: &CmvsPs2aInstructionFrame,
    mut failure: SessionError,
) -> SessionError {
    let kind = match frame {
        CmvsPs2aInstructionFrame::Control(_) => "control".to_owned(),
        CmvsPs2aInstructionFrame::ValueExpression(_) => "value expression".to_owned(),
        CmvsPs2aInstructionFrame::StackExpression(_) => "stack expression".to_owned(),
        CmvsPs2aInstructionFrame::NumericExpression(_) => "numeric expression".to_owned(),
        CmvsPs2aInstructionFrame::Command { command_id, .. } => format!("command {command_id}"),
    };
    failure.message = format!(
        "CMVS {kind} at offset {}: {}",
        frame.span().offset,
        failure.message
    );
    failure
}
