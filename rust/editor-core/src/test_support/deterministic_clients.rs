use std::cell::Cell;
use yrs::ClientID;

const FIRST_CLIENT_ID: u64 = 1;

thread_local! {
    static NEXT_CLIENT_ID: Cell<Option<u64>> = const { Cell::new(None) };
}

pub(crate) struct DeterministicClients(Option<u64>);

impl DeterministicClients {
    pub(crate) fn new() -> Self {
        Self(NEXT_CLIENT_ID.replace(Some(FIRST_CLIENT_ID)))
    }
}

impl Drop for DeterministicClients {
    fn drop(&mut self) {
        NEXT_CLIENT_ID.set(self.0);
    }
}

pub(crate) fn next_client_id() -> Option<ClientID> {
    NEXT_CLIENT_ID.get().map(|next| {
        NEXT_CLIENT_ID.set(Some(next.checked_add(1).expect("test client IDs fit u64")));
        ClientID::new(next)
    })
}
