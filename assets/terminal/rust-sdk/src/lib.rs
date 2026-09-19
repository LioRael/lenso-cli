include!("generated.rs");
use std::{cell::{Cell, RefCell}, collections::{BTreeMap, VecDeque}};
pub type Context = lenso_kernel::InvocationContext;
pub type CatalogFuture = lenso_kernel::NativeRequestFuture<CommandProviderCatalog>;
pub type ExecuteFuture = futures::future::LocalBoxFuture<'static, Result<Box<dyn lenso_kernel::NativeStreamSession>, CommandProviderExecuteInvocationError>>;
type Handler = fn(&BTreeMap<String, String>, &mut Output) -> Result<(), String>;
#[derive(Clone, Debug)]
pub struct Command { name: String, description: String, defaults: BTreeMap<String,String>, handler: Handler }
impl Command {
    pub fn new(name: &str, description: &str) -> Self { Self { name:name.into(), description:description.into(), defaults:BTreeMap::new(), handler:|_,_| Ok(()) } }
    pub fn string_arg(mut self, name: &str, default: &str) -> Self { self.defaults.insert(name.into(),default.into()); self }
    pub fn run(mut self, handler: Handler) -> Self { self.handler = handler; self }
}
#[derive(Debug, Default)]
pub struct Output { messages: VecDeque<ExecuteMessage>, bytes: usize, overflow: bool }
impl Output {
    pub fn text(&mut self, text: impl Into<String>) {
        let content = format!("{}\n", text.into()); self.bytes += content.len();
        if self.messages.len() >= 256 || self.bytes > 16*1024*1024 { self.overflow = true; return; }
        self.messages.push_back(ExecuteMessage {content,content_type:ContentType::Text,kind:OutputKind::Stdout});
    }
}
pub fn catalog(command: Command, id: &str) -> CatalogFuture {
    let parameters = command.defaults.keys().map(|key| CommandParameter {
        id:key.clone(), kind:ParameterKind::Option, long:Some(Some(key.clone())), short:None, value_name:None,
        description:String::new(), required:false, multiple:false, choices:vec![],
    }).collect();
    let response = CatalogResponse { commands:vec![CommandDefinition { id:id.into(), path:command.name.split(' ').map(str::to_owned).collect(),
        summary:command.description.clone(),description:command.description,parameters,output_formats:vec![OutputFormat::Text] }] };
    Box::pin(async move { Ok(Ok(response)) })
}
pub fn execute(command: Command, id: &str, request: ExecuteOpen) -> ExecuteFuture {
    if request.id != id { return Box::pin(async { Err(CommandProviderExecuteInvocationError::Domain(ExecuteError::NotFound)) }); }
    let args = serde_json::from_str::<BTreeMap<String,String>>(request.arguments_json.as_str());
    let Ok(args) = args else { return Box::pin(async { Err(CommandProviderExecuteInvocationError::Domain(ExecuteError::InvalidArguments)) }); };
    if args.keys().any(|key| !command.defaults.contains_key(key)) { return Box::pin(async { Err(CommandProviderExecuteInvocationError::Domain(ExecuteError::InvalidArguments)) }); }
    let mut merged = command.defaults; merged.extend(args);
    let mut output = Output::default();
    let result = (command.handler)(&merged, &mut output);
    let error = if output.overflow { Some(ExecuteError::OutputLimitExceeded) } else {
        result.err().map(|message| ExecuteError::ExecutionFailed {payload:ExecutionFailedPayload {
            reason_code:"command_failed".into(), message:message.chars().take(4096).collect(),details_json:"{}".to_owned().try_into().expect("JSON object"),
        }})
    };
    let session = Session { messages:RefCell::new(output.messages),error:RefCell::new(error),cancelled:Cell::new(false),terminal:Cell::new(false) };
    Box::pin(async move { Ok(Box::new(session) as Box<dyn lenso_kernel::NativeStreamSession>) })
}
#[derive(Debug)]
struct Session { messages:RefCell<VecDeque<ExecuteMessage>>, error:RefCell<Option<ExecuteError>>,cancelled:Cell<bool>,terminal:Cell<bool> }
impl lenso_kernel::NativeStreamSession for Session {
    fn send(&self, _: Box<dyn std::any::Any>) -> LocalBoxFuture<'static,Result<(),RuntimeFailure>> { Box::pin(async { Err(RuntimeFailure::ProtocolViolation {capability:CAPABILITY_ID}) }) }
    fn receive(&self) -> LocalBoxFuture<'static,Result<lenso_kernel::NativeStreamItem,RuntimeFailure>> {
        let event = if self.cancelled.get() || self.terminal.get() { Err(RuntimeFailure::AdmissionClosed) }
        else if let Some(message) = self.messages.borrow_mut().pop_front() { Ok(lenso_kernel::NativeStreamItem::Message(Box::new(message))) }
        else { self.terminal.set(true); Ok(lenso_kernel::NativeStreamItem::Terminal(match self.error.borrow_mut().take() {None=>Ok(()),Some(error)=>Err(Box::new(error))})) };
        Box::pin(async move { event })
    }
    fn close_send(&self) -> LocalBoxFuture<'static,Result<(),RuntimeFailure>> { Box::pin(async { Ok(()) }) }
    fn cancel(&self) { self.cancelled.set(true); self.messages.borrow_mut().clear(); }
}
