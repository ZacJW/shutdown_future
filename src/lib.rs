use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

enum ShutdownState {
    WaitingForTrigger,
    RunningAction,
    JoiningTasks,
}

/// A concurrent future for awaiting multiple triggers,
/// running clean-up, and joining tasks.
/// 
/// If any of the triggers or tasks complete, the clean-up
/// future is awaited and then all remaining tasks are awaited.
pub struct ShutdownFuture<TriggerReturn, TaskReturn, F: Future<Output = ()>> {
    triggers: Vec<Pin<Box<dyn Future<Output = TriggerReturn>>>>,
    tasks: Vec<Pin<Box<dyn Future<Output = TaskReturn>>>>,
    cleanup: Pin<Box<F>>,
    state: ShutdownState,
}

impl<TriggerReturn, TaskReturn, F: Future<Output = ()>>
    ShutdownFuture<TriggerReturn, TaskReturn, F>
{
    pub fn new(
        triggers: Vec<Pin<Box<dyn Future<Output = TriggerReturn>>>>,
        tasks: Vec<Pin<Box<dyn Future<Output = TaskReturn>>>>,
        cleanup: F,
    ) -> Self {
        Self {
            triggers,
            tasks,
            cleanup: Box::pin(cleanup),
            state: ShutdownState::WaitingForTrigger,
        }
    }
}

impl<TriggerReturn, TaskReturn, F: Future<Output = ()>> Future
    for ShutdownFuture<TriggerReturn, TaskReturn, F>
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            ShutdownState::WaitingForTrigger => {
                for trigger in self.triggers.iter_mut() {
                    if trigger.as_mut().poll(cx).is_ready() {
                        cx.waker().wake_by_ref();
                        self.state = ShutdownState::RunningAction;
                        break;
                    }
                }
                for (i, task) in self.tasks.iter_mut().enumerate() {
                    if task.as_mut().poll(cx).is_ready() {
                        #[allow(unused_must_use)]
                        {
                            self.tasks.remove(i);
                        }
                        cx.waker().wake_by_ref();
                        self.state = ShutdownState::RunningAction;
                        break;
                    }
                }
                Poll::Pending
            }
            ShutdownState::RunningAction => {
                if self.cleanup.as_mut().poll(cx).is_ready() {
                    cx.waker().wake_by_ref();
                    self.state = ShutdownState::JoiningTasks;
                }
                Poll::Pending
            }
            ShutdownState::JoiningTasks => match self.tasks.last_mut() {
                Some(task) => {
                    if task.as_mut().poll(cx).is_ready() {
                        self.tasks.pop();
                        cx.waker().wake_by_ref();
                    }
                    Poll::Pending
                }
                None => Poll::Ready(()),
            },
        }
    }
}
