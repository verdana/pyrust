#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Pending,
    Composing,
}

#[derive(Debug, Clone)]
pub struct StateMachine {
    current: State,
}

impl StateMachine {
    pub fn new(initial: State) -> Self {
        Self { current: initial }
    }

    pub fn current(&self) -> State {
        self.current
    }

    pub fn transition_to(&mut self, new: State) {
        self.current = new;
    }
}

impl From<State> for StateMachine {
    fn from(state: State) -> Self {
        Self::new(state)
    }
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new(State::Idle)
    }
}
