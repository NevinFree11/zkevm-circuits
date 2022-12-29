use crate::circuit_input_builder::{CircuitInputStateRef, ExecStep};
use crate::evm::{Opcode, OpcodeId};
use crate::Error;
use eth_types::{GethExecStep, ToAddress, ToWord, Word};
use crate::operation::{CallContextField, MemoryOp, RW};


#[derive(Debug, Copy, Clone)]
pub(crate) struct ErrorReturnDataOutOfBound;

impl Opcode for ErrorReturnDataOutOfBound {
    fn gen_associated_ops(
        state: &mut CircuitInputStateRef,
        geth_steps: &[GethExecStep],
    ) -> Result<Vec<ExecStep>, Error> {
        let geth_step = &geth_steps[0];
        let mut exec_step = state.new_step(geth_step)?;
        let next_step = if geth_steps.len() > 1 {
            Some(&geth_steps[1])
        } else {
            None
        };
        exec_step.error = state.get_step_err(geth_step, next_step).unwrap();
        // assert op code can only be RETURNDATACOPY
        assert!(geth_step.op == OpcodeId::RETURNDATACOPY);

        let memory_offset = geth_step.stack.nth_last(0)?;
        let data_offset = geth_step.stack.nth_last(1)?;
        let length = geth_step.stack.nth_last(2)?;
    
        state.stack_read(
            &mut exec_step,
            geth_step.stack.nth_last_filled(0),
            memory_offset,
        )?;
        state.stack_read(
            &mut exec_step,
            geth_step.stack.nth_last_filled(1),
            data_offset,
        )?;
        state.stack_read(&mut exec_step, geth_step.stack.nth_last_filled(2), length)?;
    
        let call_id = state.call()?.call_id;
        let call_ctx = state.call_ctx()?;
        let return_data = call_ctx.return_data.clone();
        let last_callee_return_data_length = state.call()?.last_callee_return_data_length;
        assert_eq!(
            last_callee_return_data_length as usize,
            return_data.len(),
            "callee return data size should be correct"
        );

        let end = data_offset + length;
        // check data_offset or end is u64 overflow, or 
        // last_callee_return_data_length < end
        let data_offset_overflow = data_offset > Word::from(u64::MAX);
        let end_overflow = end > Word::from(u64::MAX);
        let end_exceed_length = last_callee_return_data_length < end.as_u64();
        // one of three must hold at least one.
        assert!(data_offset_overflow | end_overflow | end_exceed_length);
        // read last callee info
        for (field, value) in [
            (
                CallContextField::LastCalleeReturnDataLength,
                return_data.len().into(),
            ),
        ] {
            state.call_context_read(&mut exec_step, call_id, field, value);
        }

        // `IsSuccess` call context operation is added in gen_restore_context_ops

        state.gen_restore_context_ops(&mut exec_step, geth_steps)?;
        state.handle_return(geth_step)?;
        Ok(vec![exec_step])
    }
}

#[cfg(test)]
mod return_tests {
    use crate::mock::BlockData;
    use eth_types::geth_types::GethData;
    use eth_types::{bytecode, word};
    use mock::test_ctx::helpers::{account_0_code_account_1_no_code, tx_from_1_to_0};
    use mock::TestContext;

    #[test]
    fn test_returndata_error() {
        // // deployed contract
        // PUSH1 0x20
        // PUSH1 0
        // PUSH1 0
        // CALLDATACOPY
        // PUSH1 0x20
        // PUSH1 0
        // RETURN
        //
        // bytecode: 0x6020600060003760206000F3
        //
        // // constructor
        // PUSH12 0x6020600060003760206000F3
        // PUSH1 0
        // MSTORE
        // PUSH1 0xC
        // PUSH1 0x14
        // RETURN
        //
        // bytecode: 0x6B6020600060003760206000F3600052600C6014F3
        let code = bytecode! {
            PUSH21(word!("6B6020600060003760206000F3600052600C6014F3"))
            PUSH1(0)
            MSTORE

            PUSH1 (0x15)
            PUSH1 (0xB)
            PUSH1 (0)
            CREATE

            PUSH1 (0x20)
            PUSH1 (0x20)
            PUSH1 (0x20)
            PUSH1 (0)
            PUSH1 (0)
            DUP6
            PUSH2 (0xFFFF)
            CALL

            PUSH1 (0x40)
            PUSH1 (0)
            PUSH1 (0x40)
            RETURNDATACOPY

            STOP
        };
        // Get the execution steps from the external tracer
        let block: GethData = TestContext::<2, 1>::new(
            None,
            account_0_code_account_1_no_code(code),
            tx_from_1_to_0,
            |block, _tx| block.number(0xcafeu64),
        )
        .unwrap()
        .into();

        let mut builder = BlockData::new_from_geth_data(block.clone()).new_circuit_input_builder();
        builder
            .handle_block(&block.eth_block, &block.geth_traces)
            .unwrap();
    }
}
