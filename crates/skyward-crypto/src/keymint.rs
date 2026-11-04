//! The phone's own key service, the only thing that can use a key the
//! hardware wrapped. A phone has one of two kinds: the newer kind, a
//! KeyMint service registered with the service directory, or the older
//! kind, a Keymaster hardware service. The newer kind is taken where the
//! phone describes one, as Android's own vold takes it.

use crate::token::Token;
use anyhow::{bail, Context, Result};
use skyward_binder::driver::{Binder, Handle};
use skyward_binder::hidl::{self, Answer, Message};
use skyward_binder::parcel::Parcel;
use skyward_binder::{service, status};

/// The newer kind, as it registers with the service directory.
pub const SERVICE: &str = "android.hardware.security.keymint.IKeyMintDevice/default";
const DEVICE: &str = "android.hardware.security.keymint.IKeyMintDevice";
const OPERATION: &str = "android.hardware.security.keymint.IKeyMintOperation";

/// The older kind, a hardware service. The newer version adds methods of
/// its own; every method used here is one the older version has too, so
/// either serves.
pub const HARDWARE_SERVICE: &str = "android.hardware.keymaster@4.1::IKeymasterDevice";
const HARDWARE_SERVICE_40: &str = "android.hardware.keymaster@4.0::IKeymasterDevice";
const HARDWARE_DEVICE: &str = "android.hardware.keymaster@4.0::IKeymasterDevice";
const INSTANCE: &str = "default";

/// How the newer kind numbers its methods.
const AIDL_GET_HARDWARE_INFO: u32 = 1;
const AIDL_UPDATE: u32 = 2;
const AIDL_FINISH: u32 = 3;
const AIDL_ABORT: u32 = 4;
const AIDL_UPGRADE_KEY: u32 = 6;
const AIDL_BEGIN: u32 = 10;
const AIDL_CONVERT_STORAGE_KEY: u32 = 13;
const AIDL_GET_KEY_CHARACTERISTICS: u32 = 14;

/// How the older kind numbers its methods. They are numbered across the
/// versions it inherits from, so these are the numbers its whole
/// interface uses.
const GET_HARDWARE_INFO: u32 = 1;
const GET_KEY_CHARACTERISTICS: u32 = 9;
const EXPORT_KEY: u32 = 10;
const UPGRADE_KEY: u32 = 12;
const BEGIN: u32 = 16;
const UPDATE: u32 = 17;
const FINISH: u32 = 18;
const ABORT: u32 = 19;

/// A key parameter as HIDL lays one out in memory: a tag, a number, and the
/// bytes some tags carry. The number is written into the union every tag
/// shares, and the bytes into the vector beside it.
const PARAMETER_BYTES: usize = 32;
const PARAMETER_BLOB: usize = 16;

/// The top of a tag says what kind of value it carries.
const TAG_KIND: i32 = -268_435_456;
const KIND_ULONG: i32 = 1_342_177_280;
const KIND_ULONG_REP: i32 = -1_610_612_736;
const KIND_DATE: i32 = 1_610_612_736;
const KIND_BYTES: i32 = -1_879_048_192;
const KIND_BIGNUM: i32 = -2_147_483_648;

/// How long the nonce of an AES-GCM key is here.
const NONCE_BYTES: usize = 12;

/// The key service's way of saying the key was made under an older build of
/// the phone's own firmware and has to be renewed before it will be used.
const KEY_REQUIRES_UPGRADE: i32 = -62;

const PURPOSE_DECRYPT: i32 = 1;
const BLOCK_MODE_GCM: i32 = 32;
const PADDING_NONE: i32 = 1;

/// What a key parameter is tagged with. The high bits say which kind of
/// value follows, which is why the numbers look arbitrary.
const TAG_PURPOSE: i32 = 536_870_913;
const TAG_BLOCK_MODE: i32 = 536_870_916;
const TAG_PADDING: i32 = 536_870_918;
const TAG_MAC_LENGTH: i32 = 805_307_371;
const TAG_APPLICATION_ID: i32 = -1_879_047_591;
const TAG_NONCE: i32 = -1_879_047_191;

/// Which arm of a parameter's value is used.
const VALUE_PADDING_MODE: i32 = 3;
const VALUE_KEY_PURPOSE: i32 = 7;
const VALUE_INTEGER: i32 = 11;
const VALUE_BLOB: i32 = 14;
const VALUE_BLOCK_MODE: i32 = 2;

/// The key formats the older kind knows.
const KEY_FORMAT_RAW: u32 = 3;

/// A key parameter as HIDL lays one out.
struct Param<'a> {
    tag: i32,
    number: u64,
    blob: &'a [u8],
}

impl<'a> Param<'a> {
    fn number(tag: i32, value: i64) -> Param<'a> {
        Param { tag, number: value as u64, blob: &[] }
    }

    fn blob(tag: i32, value: &'a [u8]) -> Param<'a> {
        Param { tag, number: 0, blob: value }
    }
}

/// Write a vector of key parameters as HIDL lays one out: the frame, the
/// array of entries, then one address for each entry's bytes.
fn write_parameters(message: &mut Message, parameters: &[Param]) {
    let mut entries = vec![0u8; parameters.len() * PARAMETER_BYTES];
    for (k, parameter) in parameters.iter().enumerate() {
        let entry = &mut entries[k * PARAMETER_BYTES..(k + 1) * PARAMETER_BYTES];
        entry[0..4].copy_from_slice(&parameter.tag.to_le_bytes());
        entry[8..16].copy_from_slice(&parameter.number.to_le_bytes());
        entry[PARAMETER_BLOB + 8..PARAMETER_BLOB + 12].copy_from_slice(&(parameter.blob.len() as u32).to_le_bytes());
    }
    let frame = message.vector_header(parameters.len() as u32);
    let array = message.buffer(&entries, Some((frame, 0)));
    for (k, parameter) in parameters.iter().enumerate() {
        message.buffer(parameter.blob, Some((array, k * PARAMETER_BYTES + PARAMETER_BLOB)));
    }
}

/// How long a hardware auth token is, and where its signature sits.
const AUTH_TOKEN_BYTES: usize = 56;
const AUTH_TOKEN_MAC: usize = 40;
/// Where a verification token's list of verified parameters and signature
/// sit.
const VERIFICATION_TOKEN_BYTES: usize = 56;
const VERIFICATION_TOKEN_PARAMETERS: usize = 16;
const VERIFICATION_TOKEN_MAC: usize = 40;

/// Write the note the secure world wrote when the user answered their
/// screen lock. A key guarded by the screen lock is refused without it, and
/// an all-zero one stands for none.
fn write_auth_token(message: &mut Message, note: Option<&Token>) {
    let mut token = vec![0u8; AUTH_TOKEN_BYTES];
    if let Some(note) = note {
        token[0..8].copy_from_slice(&note.challenge.to_le_bytes());
        token[8..16].copy_from_slice(&note.user.to_le_bytes());
        token[16..24].copy_from_slice(&note.authenticator.to_le_bytes());
        token[24..28].copy_from_slice(&note.kind.to_le_bytes());
        token[32..40].copy_from_slice(&note.when.to_le_bytes());
        token[AUTH_TOKEN_MAC + 8..AUTH_TOKEN_MAC + 12].copy_from_slice(&(note.mac.len() as u32).to_le_bytes());
    }
    let parent = message.buffer(&token, None);
    message.buffer(note.map_or(&[], |note| &note.mac), Some((parent, AUTH_TOKEN_MAC)));
}

/// Write a verification token with nothing in it: the decryptions here are
/// not confirmed on the phone's own screen.
fn write_verification_token(message: &mut Message) {
    let token = vec![0u8; VERIFICATION_TOKEN_BYTES];
    let parent = message.buffer(&token, None);
    message.buffer(&[], Some((parent, VERIFICATION_TOKEN_PARAMETERS)));
    message.buffer(&[], Some((parent, VERIFICATION_TOKEN_MAC)));
}

/// The bytes of a top-level vector: its frame, then the bytes themselves.
fn read_bytes(read: &mut Answer) -> Result<Vec<u8>> {
    let frame = read.pointed().context("read a vector's frame")?;
    if frame.len() < 16 {
        bail!("a vector's frame is {} bytes", frame.len());
    }
    Ok(read.pointed().context("read a vector's bytes")?.to_vec())
}

/// A vector's entries, then the bytes each entry's own vector points at.
fn read_parameter_list(read: &mut Answer, count: usize) -> Result<Vec<(i32, i64)>> {
    let entries = read.pointed().context("read a list of key parameters")?;
    if entries.len() != count * PARAMETER_BYTES {
        bail!("{count} key parameters take {} bytes, and {} were answered", count * PARAMETER_BYTES, entries.len());
    }
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let entry = &entries[k * PARAMETER_BYTES..(k + 1) * PARAMETER_BYTES];
        let tag = i32::from_le_bytes(entry[0..4].try_into().expect("four bytes"));
        let number = u64::from_le_bytes(entry[8..16].try_into().expect("eight bytes"));
        let size = u32::from_le_bytes(entry[PARAMETER_BLOB + 8..PARAMETER_BLOB + 12].try_into().expect("four bytes")) as usize;
        let blob = read.pointed().context("read a key parameter's bytes")?;
        if blob.len() != size {
            bail!("a key parameter's bytes are {size} long, and {} were answered", blob.len());
        }
        out.push((tag, value_of(tag, number, size)));
    }
    Ok(out)
}

/// What a parameter says, by the kind of value its tag carries.
fn value_of(tag: i32, number: u64, size: usize) -> i64 {
    match tag & TAG_KIND {
        KIND_ULONG | KIND_ULONG_REP | KIND_DATE => number as i64,
        KIND_BYTES | KIND_BIGNUM => size as i64,
        _ => number as u32 as i32 as i64,
    }
}

/// The parameters of a top-level vector, read and then stepped over.
fn read_parameters(read: &mut Answer) -> Result<Vec<(i32, i64)>> {
    let frame = read.pointed().context("read a vector's frame")?;
    if frame.len() < 16 {
        bail!("a vector's frame is {} bytes", frame.len());
    }
    let count = u32::from_le_bytes(frame[8..12].try_into().expect("four bytes")) as usize;
    read_parameter_list(read, count)
}

/// A key's characteristics: what is enforced in software and what in
/// hardware, one list after the other.
fn read_characteristics(read: &mut Answer) -> Result<Vec<(i32, i64)>> {
    let structure = read.pointed().context("read the key's characteristics")?;
    if structure.len() < 2 * 16 {
        bail!("a key's characteristics are {} bytes", structure.len());
    }
    let mut out = Vec::new();
    for field in 0..2 {
        let at = field * 16;
        let count = u32::from_le_bytes(structure[at + 8..at + 12].try_into().expect("four bytes")) as usize;
        out.extend(read_parameter_list(read, count)?);
    }
    Ok(out)
}

/// Read the key service's own error code, which is not the driver's. A key
/// made under older firmware answers one of these before it can be used.
fn read_error(read: &mut Answer, what: &str) -> Result<()> {
    let code = read.i32().with_context(|| what.to_string())?;
    match code {
        0 => Ok(()),
        code => Err(anyhow::Error::new(status::Fault::Service { code, said: String::new() }).context(what.to_string())),
    }
}

/// What the phone says about its key hardware.
pub struct Hardware {
    pub version: i32,
    pub security_level: i32,
    pub name: String,
    pub author: String,
}

/// One of the two kinds of key service.
enum Device<'a> {
    Aidl(Handle<'a>),
    Hidl { handle: Handle<'a>, version: i32 },
}

/// A decryption the key service opened. The older kind names it by a plain
/// number; the newer kind gives an object of its own.
enum Operation<'a> {
    Aidl(Handle<'a>),
    Hidl(u64),
}

/// The phone's key service, as this program talks to it.
pub struct KeyMint<'a> {
    device: Device<'a>,
}

impl<'a> KeyMint<'a> {
    /// Reach the running key service: the newer kind through `binder` where
    /// the phone describes one, the older kind through `hardware`
    /// otherwise. Say so where it is not running.
    pub fn open(binder: &'a Binder, hardware: &'a Binder) -> Result<KeyMint<'a>> {
        let not_running = |name: &str| format!("{name} is not running; the phone's key service has to be started first");
        if service::declared(binder, SERVICE)? {
            let device = service::find(binder, SERVICE)?.with_context(|| not_running(SERVICE))?;
            return Ok(KeyMint { device: Device::Aidl(device) });
        }
        for (name, version) in [(HARDWARE_SERVICE, 41), (HARDWARE_SERVICE_40, 40)] {
            if let Some(handle) = hidl::find(hardware, name, INSTANCE)? {
                return Ok(KeyMint { device: Device::Hidl { handle, version } });
            }
        }
        bail!("{}", not_running(HARDWARE_SERVICE))
    }

    pub fn hardware(&self) -> Result<Hardware> {
        match &self.device {
            Device::Aidl(device) => {
                let reply =
                    device.transact(AIDL_GET_HARDWARE_INFO, &Parcel::call(DEVICE)).context("ask the key service about itself")?;
                let mut read = reply.read();
                status::ok(&mut read).context("ask the key service about itself")?;
                let end = read.structure()?;
                let info = Hardware {
                    version: read.i32()?,
                    security_level: read.i32()?,
                    name: read.string()?,
                    author: read.string()?,
                };
                read.go(end)?;
                Ok(info)
            }
            Device::Hidl { handle, version } => {
                let reply = handle
                    .transact(GET_HARDWARE_INFO, &Message::call(HARDWARE_DEVICE))
                    .context("ask the key service about itself")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("ask the key service about itself")?;
                Ok(Hardware {
                    version: *version,
                    security_level: read.i32()?,
                    name: read.hidl_string()?,
                    author: read.hidl_string()?,
                })
            }
        }
    }

    /// Turn a key that only the storage hardware can use into the form it
    /// takes for this start of the phone. The key itself never leaves the
    /// secure world: what comes back is wrapped again, for the hardware to
    /// unwrap as it reads and writes.
    pub fn ephemeral_storage_key(&self, key: &[u8]) -> Result<Vec<u8>> {
        match self.convert(key) {
            Err(e) if status::refused_with(&e) == Some(KEY_REQUIRES_UPGRADE) => {
                // As with a key that decrypts, the key is renewed for the
                // firmware running now, as Android's keystore2 renews one
                // for vold, and the one on disk is left as it is. A storage
                // key is bound to nothing that has to be given again.
                let renewed = self.upgrade_storage(key).context("renew the storage key for the firmware running now")?;
                self.convert(&renewed)
            }
            converted => converted,
        }
    }

    fn convert(&self, key: &[u8]) -> Result<Vec<u8>> {
        match &self.device {
            Device::Aidl(device) => {
                let mut call = Parcel::call(DEVICE);
                call.blob(key);
                let reply = device.transact(AIDL_CONVERT_STORAGE_KEY, &call).context("make a storage key for this start")?;
                let mut read = reply.read();
                status::ok(&mut read).context("make a storage key for this start")?;
                read.blob().context("read the storage key")
            }
            // The older kind has no way to convert a storage key in place;
            // it exports the key raw, as Android 11's vold does, and the
            // storage hardware wraps it again.
            Device::Hidl { .. } => self.export(key).context("make a storage key for this start"),
        }
    }

    /// Export a key as its own bytes. The older kind only does this for the
    /// raw form, which is what a storage key is.
    fn export(&self, key: &[u8]) -> Result<Vec<u8>> {
        let Device::Hidl { handle, .. } = &self.device else { bail!("the key service does not export keys") };
        let mut call = Message::call(HARDWARE_DEVICE);
        call.u32(KEY_FORMAT_RAW);
        call.bytes(key);
        call.bytes(&[]);
        call.bytes(&[]);
        let reply = handle.transact(EXPORT_KEY, &call).context("export the key")?;
        let mut read = Answer::new(reply.bytes());
        read.status().context("export the key")?;
        read_error(&mut read, "export the key")?;
        read_bytes(&mut read).context("read the exported key")
    }

    /// What the key service says a key may be used for: each entry is a tag
    /// and the number or length that came with it.
    pub fn authorizations(&self, key_blob: &[u8], app_id: &[u8]) -> Result<Vec<(i32, i64)>> {
        match &self.device {
            Device::Aidl(device) => {
                let mut call = Parcel::call(DEVICE);
                call.blob(key_blob);
                call.blob(app_id);
                call.blob(&[]);
                let reply =
                    device.transact(AIDL_GET_KEY_CHARACTERISTICS, &call).context("ask what the key may be used for")?;
                let mut read = reply.read();
                status::ok(&mut read).context("ask what the key may be used for")?;
                let mut out = Vec::new();
                for _ in 0..read.i32()?.max(0) {
                    let end = read.structure()?;
                    read.i32().context("read which part of the phone holds the key")?;
                    for _ in 0..read.i32()?.max(0) {
                        let next = read.structure()?;
                        let tag = read.i32()?;
                        read.i32().context("read whether the tag has a value")?;
                        read.i32().context("read which arm of the value is set")?;
                        let value = match tag & TAG_KIND {
                            KIND_ULONG | KIND_ULONG_REP | KIND_DATE => read.i64()?,
                            KIND_BYTES | KIND_BIGNUM => read.blob()?.len() as i64,
                            _ => read.i32()? as i64,
                        };
                        out.push((tag, value));
                        read.go(next)?;
                    }
                    read.go(end)?;
                }
                Ok(out)
            }
            Device::Hidl { handle, .. } => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.bytes(key_blob);
                call.bytes(app_id);
                call.bytes(&[]);
                let reply = handle
                    .transact(GET_KEY_CHARACTERISTICS, &call)
                    .context("ask what the key may be used for")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("ask what the key may be used for")?;
                read_error(&mut read, "ask what the key may be used for")?;
                read_characteristics(&mut read)
            }
        }
    }

    /// Undo an AES-GCM encryption the key service did with `key_blob`. The
    /// service only hands back the plaintext when `app_id` is the one the
    /// key was made for, so a wrong secret fails here rather than yielding
    /// something that looks like a key. A key guarded by a screen lock
    /// takes the note the secure world wrote when it was answered.
    pub fn gcm_decrypt(&self, key_blob: &[u8], app_id: &[u8], nonce: &[u8], body: &[u8], note: Option<&Token>) -> Result<Vec<u8>> {
        let operation = match self.begin(key_blob, app_id, nonce, note) {
            Ok(operation) => operation,
            Err(e) if status::refused_with(&e) == Some(KEY_REQUIRES_UPGRADE) => {
                // The phone's firmware has moved on since this key was
                // made. Renewing it gives a key for the firmware running
                // now; the one on disk is left as it is, so nothing of the
                // phone's own is changed by reading it here.
                let renewed = self.upgrade_gcm(key_blob, app_id, nonce).context("renew the key for the firmware running now")?;
                self.begin(&renewed, app_id, nonce, note)?
            }
            Err(e) => return Err(e),
        };
        let mut out = self.update(&operation, body)?;
        out.extend_from_slice(&self.finish(&operation)?);
        Ok(out)
    }

    /// What the key service has to be told to use an AES-GCM key: how it is
    /// used, how long its tag is, what it is bound to, and the nonce it was
    /// encrypted under.
    ///
    /// What a key is bound to is named only where there is something: the
    /// secure world folds the binding into the key it unwraps the key blob
    /// with, and a binding to nothing named is not the same to it as none
    /// named at all, so naming an empty one leaves it unable to unwrap a
    /// key that was made bound to nothing.
    fn gcm_parameters<'b>(app_id: &'b [u8], nonce: &'b [u8]) -> Vec<Param<'b>> {
        let mut parameters = Vec::new();
        parameters.push(Param::number(TAG_BLOCK_MODE, BLOCK_MODE_GCM as i64));
        parameters.push(Param::number(TAG_PADDING, PADDING_NONE as i64));
        parameters.push(Param::number(TAG_MAC_LENGTH, 128));
        if !app_id.is_empty() {
            parameters.push(Param::blob(TAG_APPLICATION_ID, app_id));
        }
        parameters.push(Param::blob(TAG_NONCE, nonce));
        parameters.push(Param::number(TAG_PURPOSE, PURPOSE_DECRYPT as i64));
        parameters
    }

    /// What the newer kind has to be told to use an AES-GCM key, written
    /// the way its own layout takes them.
    fn gcm_params(call: &mut Parcel, app_id: &[u8], nonce: &[u8]) {
        call.i32(if app_id.is_empty() { 5 } else { 6 });
        parameter(call, TAG_BLOCK_MODE, Value::Enum(VALUE_BLOCK_MODE, BLOCK_MODE_GCM));
        parameter(call, TAG_PADDING, Value::Enum(VALUE_PADDING_MODE, PADDING_NONE));
        parameter(call, TAG_MAC_LENGTH, Value::Integer(128));
        if !app_id.is_empty() {
            parameter(call, TAG_APPLICATION_ID, Value::Blob(app_id));
        }
        parameter(call, TAG_NONCE, Value::Blob(nonce));
        parameter(call, TAG_PURPOSE, Value::Enum(VALUE_KEY_PURPOSE, PURPOSE_DECRYPT));
    }

    /// Open a decryption with the key service and keep hold of it.
    fn begin(&self, key_blob: &[u8], app_id: &[u8], nonce: &[u8], note: Option<&Token>) -> Result<Operation<'a>> {
        match &self.device {
            Device::Aidl(device) => {
                let mut call = Parcel::call(DEVICE);
                call.i32(PURPOSE_DECRYPT);
                call.blob(key_blob);
                Self::gcm_params(&mut call, app_id, nonce);
                // A key guarded by the screen lock takes the note the secure
                // world wrote when the user answered it; one that is not
                // guarded takes nothing, and is refused nothing for carrying
                // a note either.
                match note {
                    Some(note) => vouched(&mut call, note),
                    None => call.none(),
                }

                // The decryption the service opens outlives the reply that
                // named it, so take a share of it before the reply's room
                // goes back.
                let reply = device.transact(AIDL_BEGIN, &call).context("start a decryption")?;
                let mut read = reply.read();
                status::ok(&mut read).context("start a decryption")?;
                read.structure().context("read what the key service answered")?;
                read.i64().context("read the decryption's challenge")?;
                let params = read.i32().context("read how many parameters the decryption came back with")?;
                for _ in 0..params.max(0) {
                    let end = read.structure().context("read a parameter the decryption came back with")?;
                    read.go(end)?;
                }
                match read.object().context("read the decryption the key service opened")? {
                    Some(h) => Ok(Operation::Aidl(Handle::hold(reply.binder(), h)?)),
                    None => bail!("the key service opened no decryption"),
                }
            }
            Device::Hidl { handle, .. } => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.u32(PURPOSE_DECRYPT as u32);
                call.bytes(key_blob);
                write_parameters(&mut call, &Self::gcm_parameters(app_id, nonce));
                write_auth_token(&mut call, note);
                let reply = handle.transact(BEGIN, &call).context("start a decryption")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("start a decryption")?;
                read_error(&mut read, "start a decryption")?;
                read_parameters(&mut read).context("read what the key service answered")?;
                let operation = read.u64().context("read the decryption the key service opened")?;
                Ok(Operation::Hidl(operation))
            }
        }
    }

    /// A key for the firmware running now, from one made under an older
    /// build. What it is bound to has to be given again, or the service
    /// cannot tell it is the right key.
    fn upgrade_storage(&self, key_blob: &[u8]) -> Result<Vec<u8>> {
        match &self.device {
            Device::Aidl(device) => {
                let mut call = Parcel::call(DEVICE);
                call.blob(key_blob);
                call.i32(0);
                let reply = device.transact(AIDL_UPGRADE_KEY, &call).context("renew the key")?;
                let mut read = reply.read();
                status::ok(&mut read).context("renew the key")?;
                read.blob().context("read the renewed key")
            }
            Device::Hidl { handle, .. } => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.bytes(key_blob);
                write_parameters(&mut call, &[]);
                let reply = handle.transact(UPGRADE_KEY, &call).context("renew the key")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("renew the key")?;
                read_error(&mut read, "renew the key")?;
                read_bytes(&mut read).context("read the renewed key")
            }
        }
    }

    fn upgrade_gcm(&self, key_blob: &[u8], app_id: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        match &self.device {
            Device::Aidl(device) => {
                let mut call = Parcel::call(DEVICE);
                call.blob(key_blob);
                Self::gcm_params(&mut call, app_id, nonce);
                let reply = device.transact(AIDL_UPGRADE_KEY, &call).context("renew the key")?;
                let mut read = reply.read();
                status::ok(&mut read).context("renew the key")?;
                read.blob().context("read the renewed key")
            }
            Device::Hidl { handle, .. } => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.bytes(key_blob);
                write_parameters(&mut call, &Self::gcm_parameters(app_id, nonce));
                let reply = handle.transact(UPGRADE_KEY, &call).context("renew the key")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("renew the key")?;
                read_error(&mut read, "renew the key")?;
                read_bytes(&mut read).context("read the renewed key")
            }
        }
    }

    fn update(&self, operation: &Operation, input: &[u8]) -> Result<Vec<u8>> {
        match (&self.device, operation) {
            (Device::Aidl(_), Operation::Aidl(handle)) => {
                let mut call = Parcel::call(OPERATION);
                call.blob(input);
                call.none();
                call.none();
                let reply = handle.transact(AIDL_UPDATE, &call).context("feed the decryption")?;
                let mut read = reply.read();
                status::ok(&mut read).context("feed the decryption")?;
                read.blob().context("read what the decryption gave back")
            }
            (Device::Hidl { handle, .. }, Operation::Hidl(operation)) => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.u64(*operation);
                write_parameters(&mut call, &[]);
                call.bytes(input);
                write_auth_token(&mut call, None);
                write_verification_token(&mut call);
                let reply = handle.transact(UPDATE, &call).context("feed the decryption")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("feed the decryption")?;
                read_error(&mut read, "feed the decryption")?;
                read.u32().context("read how much of the input was taken")?;
                read_parameters(&mut read).context("read what the decryption answered")?;
                read_bytes(&mut read).context("read what the decryption gave back")
            }
            _ => bail!("the decryption belongs to another kind of key service"),
        }
    }

    fn finish(&self, operation: &Operation) -> Result<Vec<u8>> {
        match (&self.device, operation) {
            (Device::Aidl(_), Operation::Aidl(handle)) => {
                let mut call = Parcel::call(OPERATION);
                call.opt_blob(None);
                call.opt_blob(None);
                call.none();
                call.none();
                call.opt_blob(None);
                let reply = handle.transact(AIDL_FINISH, &call).context("close the decryption")?;
                let mut read = reply.read();
                status::ok(&mut read).context("close the decryption, which is where a wrong key is refused")?;
                read.blob().context("read what the decryption ended with")
            }
            (Device::Hidl { handle, .. }, Operation::Hidl(operation)) => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.u64(*operation);
                write_parameters(&mut call, &[]);
                call.bytes(&[]);
                call.bytes(&[]);
                write_auth_token(&mut call, None);
                write_verification_token(&mut call);
                let reply = handle.transact(FINISH, &call).context("close the decryption")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("close the decryption")?;
                read_error(&mut read, "close the decryption, which is where a wrong key is refused")?;
                read_parameters(&mut read).context("read what the decryption answered")?;
                read_bytes(&mut read).context("read what the decryption ended with")
            }
            _ => bail!("the decryption belongs to another kind of key service"),
        }
    }

    fn abort(&self, operation: &Operation) -> Result<()> {
        match (&self.device, operation) {
            (Device::Aidl(_), Operation::Aidl(handle)) => {
                let reply = handle.transact(AIDL_ABORT, &Parcel::call(OPERATION)).context("drop the decryption")?;
                status::ok(&mut reply.read()).context("drop the decryption")
            }
            (Device::Hidl { handle, .. }, Operation::Hidl(operation)) => {
                let mut call = Message::call(HARDWARE_DEVICE);
                call.u64(*operation);
                let reply = handle.transact(ABORT, &call).context("drop the decryption")?;
                let mut read = Answer::new(reply.bytes());
                read.status().context("drop the decryption")?;
                read_error(&mut read, "drop the decryption")
            }
            _ => bail!("the decryption belongs to another kind of key service"),
        }
    }

    /// Whether the secure world will unwrap the key at all: a decryption is
    /// opened on it under a nonce of nothing and dropped again, which reads
    /// nothing and changes nothing. It tells a key the secure world cannot
    /// unwrap, as one made under another build or bound to something not
    /// given back to it is, apart from one that merely decrypts to nothing.
    pub fn usable(&self, key_blob: &[u8], app_id: &[u8]) -> Result<()> {
        let operation = self.begin(key_blob, app_id, &[0; NONCE_BYTES], None)?;
        self.abort(&operation)
    }
}

/// The tag's value and which arm it is written as.
enum Value<'a> {
    Enum(i32, i32),
    Integer(i32),
    Blob(&'a [u8]),
}

fn parameter(p: &mut Parcel, tag: i32, value: Value) {
    // Each parameter is a structure of its own: it says it is there, then
    // how long it is, so the far side can step over what it does not know.
    p.i32(1);
    let start = p.here();
    p.i32(0);
    p.i32(tag);
    // The value is a structure of its own too, and says so the same way,
    // though it carries no length because only one arm of it is ever set.
    p.i32(1);
    match value {
        Value::Enum(arm, v) => {
            p.i32(arm);
            p.i32(v);
        }
        Value::Integer(v) => {
            p.i32(VALUE_INTEGER);
            p.i32(v);
        }
        Value::Blob(v) => {
            p.i32(VALUE_BLOB);
            p.blob(v);
        }
    }
    let size = (p.here() - start) as i32;
    p.fill(start, size);
}

/// Hand over the note the secure world wrote when the user answered their
/// screen lock. A key guarded by the screen lock is refused without it.
fn vouched(p: &mut Parcel, note: &Token) {
    p.i32(1);
    let start = p.here();
    p.i32(0);
    p.i64(note.challenge);
    p.i64(note.user);
    p.i64(note.authenticator);
    p.i32(note.kind);
    // The time it was written is a structure of its own.
    p.i32(1);
    let when = p.here();
    p.i32(0);
    p.i64(note.when);
    let size = (p.here() - when) as i32;
    p.fill(when, size);
    p.blob(&note.mac);
    let size = (p.here() - start) as i32;
    p.fill(start, size);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_list_is_written_as_a_frame_an_array_and_one_address_per_entry() {
        let mut m = Message::call(HARDWARE_DEVICE);
        write_parameters(&mut m, &[Param::number(TAG_BLOCK_MODE, BLOCK_MODE_GCM as i64), Param::blob(TAG_NONCE, &[1, 2, 3])]);
        // The frame, the array, and the nonce: the number entry carries no
        // bytes, so it is written as nothing and is not an address the
        // driver has to carry.
        let objects = m.addresses.len();
        assert_eq!(objects, 3, "the frame, the array, and the nonce");
        assert_eq!(m.pointed, 16 + (2 * PARAMETER_BYTES as u64).next_multiple_of(8) + 8);
    }

    #[test]
    fn a_key_parameter_says_its_tag_its_number_and_where_its_bytes_are() {
        let mut m = Message::call(HARDWARE_DEVICE);
        write_parameters(&mut m, &[Param::blob(TAG_APPLICATION_ID, &[9; 4])]);
        // The frame is the first address, the array the second, and the
        // bytes the third.
        let frame = &m.data[m.addresses[0] as usize..];
        let array_at = u64::from_le_bytes(frame[8..16].try_into().unwrap());
        assert_eq!(u32::from_le_bytes(frame[16..20].try_into().unwrap()), 1, "one parameter");
        let array = unsafe { std::slice::from_raw_parts(array_at as *const u8, PARAMETER_BYTES) };
        assert_eq!(i32::from_le_bytes(array[0..4].try_into().unwrap()), TAG_APPLICATION_ID);
        assert_eq!(u32::from_le_bytes(array[PARAMETER_BLOB + 8..PARAMETER_BLOB + 12].try_into().unwrap()), 4);
        let entry = &m.data[m.addresses[1] as usize..];
        assert_eq!(u64::from_le_bytes(entry[32..40].try_into().unwrap()), PARAMETER_BLOB as u64, "the bytes sit in the entry");
        let bytes = &m.data[m.addresses[2] as usize..];
        assert_eq!(u64::from_le_bytes(bytes[24..32].try_into().unwrap()), 1, "inside the array");
    }

    #[test]
    fn an_empty_parameter_list_is_a_frame_pointing_at_nothing() {
        let mut m = Message::call(HARDWARE_DEVICE);
        write_parameters(&mut m, &[]);
        assert_eq!(m.addresses.len(), 1, "only the frame");
        assert_eq!(m.pointed, 16);
    }

    #[test]
    fn a_zero_auth_token_is_a_structure_and_two_addresses_of_nothing() {
        let mut m = Message::call(HARDWARE_DEVICE);
        write_auth_token(&mut m, None);
        assert_eq!(m.addresses.len(), 1, "the token itself");
        assert_eq!(m.pointed, AUTH_TOKEN_BYTES as u64);
    }

    #[test]
    fn a_note_is_written_where_the_auth_token_holds_it() {
        let mut m = Message::call(HARDWARE_DEVICE);
        let note = Token { challenge: 7, user: 9, authenticator: 11, kind: 1, when: 13, mac: vec![0xAB; 32] };
        write_auth_token(&mut m, Some(&note));
        let token_at = u64::from_le_bytes(m.data[m.addresses[0] as usize + 8..m.addresses[0] as usize + 16].try_into().unwrap());
        let token = unsafe { std::slice::from_raw_parts(token_at as *const u8, AUTH_TOKEN_BYTES) };
        assert_eq!(u64::from_le_bytes(token[0..8].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(token[8..16].try_into().unwrap()), 9);
        assert_eq!(u64::from_le_bytes(token[16..24].try_into().unwrap()), 11);
        assert_eq!(i32::from_le_bytes(token[24..28].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(token[32..40].try_into().unwrap()), 13);
        assert_eq!(u32::from_le_bytes(token[AUTH_TOKEN_MAC + 8..AUTH_TOKEN_MAC + 12].try_into().unwrap()), 32);
    }

    #[test]
    fn a_key_parameter_says_what_kind_of_value_its_tag_carries() {
        assert_eq!(value_of(TAG_MAC_LENGTH, 128, 0), 128);
        assert_eq!(value_of(TAG_NONCE, 0, 12), 12, "bytes are their length");
        assert_eq!(value_of(TAG_PURPOSE, PURPOSE_DECRYPT as u64, 0), 1);
    }

    #[test]
    fn a_key_services_own_refusal_can_be_acted_on() {
        let mut reply = 0i32.to_le_bytes().to_vec();
        reply.extend_from_slice(&KEY_REQUIRES_UPGRADE.to_le_bytes());
        let mut read = Answer::new(&reply);
        read.status().unwrap();
        let e = read_error(&mut read, "start a decryption").unwrap_err();
        assert_eq!(status::refused_with(&e), Some(KEY_REQUIRES_UPGRADE));
        assert!(format!("{e:#}").contains("-62"), "{e:#}");
    }
}
