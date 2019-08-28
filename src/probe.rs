use async_trait::async_trait;

#[async_trait]
pub trait Probe {
    type Msg: Send;
    type Pay: Clone + Send;
    
    async fn event(&mut self, evt: Self::Msg);
    fn payload(&self) -> &Self::Pay;
}

#[async_trait]
pub trait ProbeReceive {
    type Msg: Send;

    async fn recv(&mut self) -> Self::Msg;
    fn reset_timer(&mut self);
    fn last_event_milliseconds(&self) -> u64;
    fn last_event_seconds(&self) -> u64;
}

/// The channel module provides an futures::channel::mpsc::unbounded() based Probe
/// that is suitable for use in a single, local application.
/// This Probe cannot be serialized.
pub mod channel {
    use crate::probe::{Probe, ProbeReceive};

    use chrono::prelude::*;
    use async_trait::async_trait;
    use futures::{channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender}, prelude::*};

    pub fn probe<T: Send>() -> (ChannelProbe<(), T>, ChannelProbeReceive<T>) {
        probe_with_payload(())
    }

    pub fn probe_with_payload<P: Clone + Send, T: Send>(payload: P) -> (ChannelProbe<P, T>, ChannelProbeReceive<T>) {
        let (tx, rx) = unbounded::<T>();

        let probe = ChannelProbe {
            payload: Some(payload),
            tx: tx.clone()
        };

        let receiver = ChannelProbeReceive {
            rx,
            tx,
            timer_start: Utc::now()
        };

        (probe, receiver)
    }

    #[derive(Clone, Debug)]
    pub struct ChannelProbe<P, T> {
        payload: Option<P>,
        tx: UnboundedSender<T>,
    }

    #[async_trait]
    impl<P, T> Probe for ChannelProbe<P, T> 
        where P: Clone + Send, T: Send {
            type Msg = T;
            type Pay = P;

            async fn event(&mut self, evt: T) {
                drop(self.tx.send(evt).await);
            }

            fn payload(&self) -> &P {
                &self.payload.as_ref().unwrap()
            }
    }

    #[async_trait]
    impl<P, T> Probe for Option<ChannelProbe<P, T>>
        where P: Clone + Send + Sync, T: Send {
            type Msg = T;
            type Pay = P;

            async fn event(&mut self, evt: T) {
                let probe_option = self.as_mut();
                let probe = probe_option.unwrap();
                drop(probe.tx.send(evt).await);
            }

            fn payload(&self) -> &P {
                &self.as_ref().unwrap().payload.as_ref().unwrap()
            }
    }

    #[allow(dead_code)]
    pub struct ChannelProbeReceive<T> {
        rx: UnboundedReceiver<T>,
        tx: UnboundedSender<T>,
        timer_start: DateTime<Utc>,
    }

    #[async_trait]
    impl<T: Send> ProbeReceive for ChannelProbeReceive<T> {
        type Msg = T;

        async fn recv(&mut self) -> T {
            self.rx.next().await.unwrap()
        }

        fn reset_timer(&mut self) {
            self.timer_start = Utc::now();
        }

        fn last_event_milliseconds(&self) -> u64 {
            let now = Utc::now();
            now.time().signed_duration_since(self.timer_start.time()).num_milliseconds() as u64
        }

        fn last_event_seconds(&self) -> u64 {
            let now = Utc::now();
            now.time().signed_duration_since(self.timer_start.time()).num_seconds() as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::probe::{Probe, ProbeReceive};
    use crate::probe::channel::{probe, probe_with_payload};
    use futures::executor::block_on;

    #[test]
    fn chan_probe() {
        block_on(async {
            let (mut probe, mut listen) = probe();

            probe.event("some event").await;

            assert_eq!(listen.recv().await, "some event");
        });
    }

    #[test]
    fn chan_probe_with_payload() {
        block_on(async {
            let payload = "test data".to_string();
            let (mut probe, mut listen) = probe_with_payload(payload);

            // only event the expected result if the payload is what we expect
            if probe.payload() == "test data" {
                probe.event("data received").await;
            } else {
                probe.event("").await;
            }

            assert_eq!(listen.recv().await, "data received");
        });
    }
}


/// Macros that provide easy use of Probes
pub mod macros {
    /// Mimicks assert_eq!
    /// Performs an assert_eq! on the first event sent by the probe.
    #[macro_export]
    macro_rules! p_assert_eq {
        ($listen:expr, $expected:expr) => {
            assert_eq!($listen.recv().await, $expected);
        };
    }

    /// Evaluates events sent from the probe with a vector of expected events.
    /// If an unexpected event is received it will assert!(false).
    /// Each good event is removed from the expected vector.
    /// The assertion is complete when there are no more expected events.
    #[macro_export]
    macro_rules! p_assert_events {
        ($listen:expr, $expected:expr) => {
            let mut expected = $expected.clone(); // so we don't need the original mutable
            
            loop {
                let item = $listen.recv().await;
                match expected.iter().position(|x| x == &item) {
                    Some(pos) => {
                        expected.remove(pos);
                        if expected.len() == 0 {
                            break;
                        }
                    }
                    _ => {
                        // probe has received an unexpected event value
                        assert!(false);
                    }
                }
            }
        };
    }

    #[macro_export]
    macro_rules! p_timer {
        ($listen:expr) => {
            $listen.last_event_milliseconds()
        };
    }

    #[cfg(test)]
    mod tests {
        use futures::executor::block_on;
        use crate::probe::{Probe, ProbeReceive};
        use crate::probe::channel::probe;

        #[test]
        fn p_assert_eq() {
            block_on(async {
                let (mut probe, mut listen) = probe();

                probe.event("test".to_string()).await;

                p_assert_eq!(listen, "test".to_string());
            });
        }

        #[test]
        fn p_assert_events() {
            block_on(async {
                let (mut probe, mut listen) = probe();

                let expected = vec!["event_1", "event_2", "event_3"];
                probe.event("event_1").await;
                probe.event("event_2").await;
                probe.event("event_3").await;

                p_assert_events!(listen, expected);
            });
        }

        #[test]
        fn p_timer() {
            block_on(async {
                let (mut probe, listen) = probe();
                probe.event("event_3").await;

                println!("Milliseconds: {}", p_timer!(listen));
            });
        }

    }
}
