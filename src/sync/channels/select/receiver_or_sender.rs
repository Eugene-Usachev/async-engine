// TODO r
// use crate::sync::{Channel, LocalChannel};
//
// macro_rules! generate_receiver_or_sender {
//     ($($name:ident, $channel:ident),*) => {
//         $(
//             pub(crate) enum $name {
//                 Receiver(&'static $channel<()>, *const (), usize),
//                 Sender(&'static $channel<()>, *mut (), usize),
//             }
//
//             impl $name {
//                 pub(crate) fn new_receiver<T>(channel: &$channel<T>, data: *const ()) -> Self {
//                     Self::Receiver(unsafe { std::mem::transmute(channel) }, data, std::mem::size_of::<T>())
//                 }
//
//                 pub(crate) fn new_sender<T>(channel: &$channel<T>, slot: *mut ()) -> Self {
//                     Self::Sender(unsafe { std::mem::transmute(channel) }, slot, std::mem::size_of::<T>())
//                 }
//
//                 pub(crate) fn do_local_operation(self) {
//                     match self {
//                         Self::Receiver(channel, data, size) => {
//                             $crate::sync::channels::SelectReceiver::recv_or_subscribe()
//                         }
//                     }
//                 }
//             }
//         )*
//     };
// }
//
// generate_receiver_or_sender!(
//     LocalReceiverOrSender,
//     LocalChannel,
//     SharedReceiverOrSender,
//     Channel
// );
