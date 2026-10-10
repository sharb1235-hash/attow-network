use crate::RunId;
use chronos_codec::ObjectId;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Task {
    pub run: RunId,
    pub node: ObjectId,
}

#[derive(Default)]
pub(crate) struct FairQueue {
    tenants: VecDeque<ObjectId>,
    tasks: BTreeMap<ObjectId, VecDeque<Task>>,
    len: usize,
}
impl FairQueue {
    pub fn push(&mut self, tenant: ObjectId, task: Task) {
        if !self.tasks.contains_key(&tenant) {
            self.tenants.push_back(tenant);
        }
        self.tasks.entry(tenant).or_default().push_back(task);
        self.len += 1;
    }
    pub fn pop(&mut self) -> Option<(ObjectId, Task)> {
        let tenant = self.tenants.pop_front()?;
        let tasks = self.tasks.get_mut(&tenant)?;
        let task = tasks.pop_front()?;
        if tasks.is_empty() {
            self.tasks.remove(&tenant);
        } else {
            self.tenants.push_back(tenant);
        }
        self.len -= 1;
        Some((tenant, task))
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn remove_run(&mut self, run: RunId) {
        self.tasks.retain(|_, tasks| {
            tasks.retain(|task| task.run != run);
            !tasks.is_empty()
        });
        self.tenants
            .retain(|tenant| self.tasks.contains_key(tenant));
        self.len = self.tasks.values().map(VecDeque::len).sum();
    }
}
