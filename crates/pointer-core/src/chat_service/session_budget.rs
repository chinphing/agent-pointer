/// Per-pool cap: `max` tool cycles; pool is either single-agent or Supervisor (sub-agents) for the conversation.
#[derive(Debug)]
pub(crate) struct SessionToolBudget {
    max: u32,
    used_before_request: u32,
    consumed_this_request: u32,
}

impl SessionToolBudget {
    pub(crate) fn new(max: u32, used_before_request: u32) -> Self {
        Self {
            max,
            used_before_request,
            consumed_this_request: 0,
        }
    }

    pub(crate) fn cap(&self) -> u32 {
        self.max
    }

    pub(crate) fn record_tool_cycle(&mut self) {
        self.consumed_this_request = self.consumed_this_request.saturating_add(1);
    }

    pub(crate) fn remaining(&self) -> u32 {
        self.max
            .saturating_sub(self.used_before_request)
            .saturating_sub(self.consumed_this_request)
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        self.used_before_request
            .saturating_add(self.consumed_this_request)
            >= self.max
    }

    pub(crate) fn sync_out(&self, out: &mut u32) {
        *out = self.consumed_this_request;
    }
}
