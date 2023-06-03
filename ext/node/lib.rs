// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use deno_core::error::AnyError;
use deno_core::located_script_name;
use deno_core::op;
use deno_core::serde_json;
use deno_core::url::Url;
use deno_core::JsRuntime;
use deno_core::ModuleSpecifier;
use deno_fs::sync::MaybeSend;
use deno_fs::sync::MaybeSync;
use deno_npm::resolution::PackageReqNotFoundError;
use deno_npm::NpmPackageId;
use deno_semver::npm::NpmPackageNv;
use deno_semver::npm::NpmPackageNvReference;
use deno_semver::npm::NpmPackageReq;
use deno_semver::npm::NpmPackageReqReference;
use once_cell::sync::Lazy;

pub mod analyze;
pub mod errors;
mod ops;
mod package_json;
mod path;
mod polyfill;
mod resolution;

pub use package_json::PackageJson;
pub use path::PathClean;
pub use polyfill::is_builtin_node_module;
pub use polyfill::NodeModulePolyfill;
pub use polyfill::SUPPORTED_BUILTIN_NODE_MODULES;
pub use resolution::NodeModuleKind;
pub use resolution::NodeResolution;
pub use resolution::NodeResolutionMode;
pub use resolution::NodeResolver;

pub trait NodePermissions {
  fn check_net_url(
    &mut self,
    url: &Url,
    api_name: &str,
  ) -> Result<(), AnyError>;
  fn check_read(&self, path: &Path) -> Result<(), AnyError>;
}

pub(crate) struct AllowAllNodePermissions;

impl NodePermissions for AllowAllNodePermissions {
  fn check_net_url(
    &mut self,
    _url: &Url,
    _api_name: &str,
  ) -> Result<(), AnyError> {
    Ok(())
  }
  fn check_read(&self, _path: &Path) -> Result<(), AnyError> {
    Ok(())
  }
}

#[allow(clippy::disallowed_types)]
pub type NpmResolverRc = deno_fs::sync::MaybeArc<dyn NpmResolver>;

pub trait NpmResolver: std::fmt::Debug + MaybeSend + MaybeSync {
  /// Resolves an npm package folder path from an npm package referrer.
  fn resolve_package_folder_from_package(
    &self,
    specifier: &str,
    referrer: &ModuleSpecifier,
    mode: NodeResolutionMode,
  ) -> Result<PathBuf, AnyError>;

  /// Resolves the npm package folder path from the specified path.
  fn resolve_package_folder_from_path(
    &self,
    path: &Path,
  ) -> Result<PathBuf, AnyError>;

  /// Resolves an npm package folder path from a Deno module.
  fn resolve_package_folder_from_deno_module(
    &self,
    pkg_nv: &NpmPackageNv,
  ) -> Result<PathBuf, AnyError>;

  fn resolve_pkg_id_from_pkg_req(
    &self,
    req: &NpmPackageReq,
  ) -> Result<NpmPackageId, PackageReqNotFoundError>;

  fn resolve_nv_ref_from_pkg_req_ref(
    &self,
    req_ref: &NpmPackageReqReference,
  ) -> Result<NpmPackageNvReference, PackageReqNotFoundError>;

  fn in_npm_package(&self, specifier: &ModuleSpecifier) -> bool;

  fn in_npm_package_at_path(&self, path: &Path) -> bool {
    let specifier =
      match ModuleSpecifier::from_file_path(path.to_path_buf().clean()) {
        Ok(p) => p,
        Err(_) => return false,
      };
    self.in_npm_package(&specifier)
  }

  fn ensure_read_permission(
    &self,
    permissions: &dyn NodePermissions,
    path: &Path,
  ) -> Result<(), AnyError>;
}

pub const NODE_GLOBAL_THIS_NAME: &str = env!("NODE_GLOBAL_THIS_NAME");

pub static NODE_ENV_VAR_ALLOWLIST: Lazy<HashSet<String>> = Lazy::new(|| {
  // The full list of environment variables supported by Node.js is available
  // at https://nodejs.org/api/cli.html#environment-variables
  let mut set = HashSet::new();
  set.insert("NODE_DEBUG".to_string());
  set.insert("NODE_OPTIONS".to_string());
  set
});

#[op]
fn op_node_build_os() -> String {
  std::env::var("TARGET")
    .unwrap()
    .split('-')
    .nth(2)
    .unwrap()
    .to_string()
}

deno_core::extension!(deno_node,
  deps = [ deno_io, deno_fs ],
  parameters = [P: NodePermissions],
  ops = [
    ops::crypto::op_node_create_decipheriv,
    ops::crypto::op_node_cipheriv_encrypt,
    ops::crypto::op_node_cipheriv_final,
    ops::crypto::op_node_create_cipheriv,
    ops::crypto::op_node_create_hash,
    ops::crypto::op_node_decipheriv_decrypt,
    ops::crypto::op_node_decipheriv_final,
    ops::crypto::op_node_hash_update,
    ops::crypto::op_node_hash_update_str,
    ops::crypto::op_node_hash_digest,
    ops::crypto::op_node_hash_digest_hex,
    ops::crypto::op_node_hash_clone,
    ops::crypto::op_node_private_encrypt,
    ops::crypto::op_node_private_decrypt,
    ops::crypto::op_node_public_encrypt,
    ops::crypto::op_node_check_prime,
    ops::crypto::op_node_check_prime_async,
    ops::crypto::op_node_check_prime_bytes,
    ops::crypto::op_node_check_prime_bytes_async,
    ops::crypto::op_node_gen_prime,
    ops::crypto::op_node_gen_prime_async,
    ops::crypto::op_node_pbkdf2,
    ops::crypto::op_node_pbkdf2_async,
    ops::crypto::op_node_hkdf,
    ops::crypto::op_node_hkdf_async,
    ops::crypto::op_node_generate_secret,
    ops::crypto::op_node_generate_secret_async,
    ops::crypto::op_node_sign,
    ops::crypto::op_node_generate_rsa,
    ops::crypto::op_node_generate_rsa_async,
    ops::crypto::op_node_dsa_generate,
    ops::crypto::op_node_dsa_generate_async,
    ops::crypto::op_node_ec_generate,
    ops::crypto::op_node_ec_generate_async,
    ops::crypto::op_node_ed25519_generate,
    ops::crypto::op_node_ed25519_generate_async,
    ops::crypto::op_node_x25519_generate,
    ops::crypto::op_node_x25519_generate_async,
    ops::crypto::op_node_dh_generate_group,
    ops::crypto::op_node_dh_generate_group_async,
    ops::crypto::op_node_dh_generate,
    ops::crypto::op_node_dh_generate2,
    ops::crypto::op_node_dh_compute_secret,
    ops::crypto::op_node_dh_generate_async,
    ops::crypto::op_node_verify,
    ops::crypto::op_node_random_int,
    ops::crypto::op_node_scrypt_sync,
    ops::crypto::op_node_scrypt_async,
    ops::crypto::op_node_ecdh_generate_keys,
    ops::crypto::op_node_ecdh_compute_secret,
    ops::crypto::op_node_ecdh_compute_public_key,
    ops::crypto::x509::op_node_x509_parse,
    ops::crypto::x509::op_node_x509_ca,
    ops::crypto::x509::op_node_x509_check_email,
    ops::crypto::x509::op_node_x509_fingerprint,
    ops::crypto::x509::op_node_x509_fingerprint256,
    ops::crypto::x509::op_node_x509_fingerprint512,
    ops::crypto::x509::op_node_x509_get_issuer,
    ops::crypto::x509::op_node_x509_get_subject,
    ops::crypto::x509::op_node_x509_get_valid_from,
    ops::crypto::x509::op_node_x509_get_valid_to,
    ops::crypto::x509::op_node_x509_get_serial_number,
    ops::crypto::x509::op_node_x509_key_usage,
    ops::winerror::op_node_sys_to_uv_error,
    ops::v8::op_v8_cached_data_version_tag,
    ops::v8::op_v8_get_heap_statistics,
    ops::idna::op_node_idna_domain_to_ascii,
    ops::idna::op_node_idna_domain_to_unicode,
    ops::idna::op_node_idna_punycode_decode,
    ops::idna::op_node_idna_punycode_encode,
    ops::zlib::op_zlib_new,
    ops::zlib::op_zlib_close,
    ops::zlib::op_zlib_close_if_pending,
    ops::zlib::op_zlib_write,
    ops::zlib::op_zlib_write_async,
    ops::zlib::op_zlib_init,
    ops::zlib::op_zlib_reset,
    ops::http::op_node_http_request<P>,
    op_node_build_os,
    ops::require::op_require_init_paths,
    ops::require::op_require_node_module_paths<P>,
    ops::require::op_require_proxy_path,
    ops::require::op_require_is_deno_dir_package,
    ops::require::op_require_resolve_deno_dir,
    ops::require::op_require_is_request_relative,
    ops::require::op_require_resolve_lookup_paths,
    ops::require::op_require_try_self_parent_path<P>,
    ops::require::op_require_try_self<P>,
    ops::require::op_require_real_path<P>,
    ops::require::op_require_path_is_absolute,
    ops::require::op_require_path_dirname,
    ops::require::op_require_stat<P>,
    ops::require::op_require_path_resolve,
    ops::require::op_require_path_basename,
    ops::require::op_require_read_file<P>,
    ops::require::op_require_as_file_path,
    ops::require::op_require_resolve_exports<P>,
    ops::require::op_require_read_closest_package_json<P>,
    ops::require::op_require_read_package_scope<P>,
    ops::require::op_require_package_imports_resolve<P>,
    ops::require::op_require_break_on_next_statement,
  ],
  esm_entry_point = "ext:deno_node/02_init.js",
  esm = [
    dir "polyfills",
    "00_globals.js",
    "01_require.js",
    "02_init.js",
    "_events.mjs",
    "_fs/_fs_access.ts",
    "_fs/_fs_appendFile.ts",
    "_fs/_fs_chmod.ts",
    "_fs/_fs_chown.ts",
    "_fs/_fs_close.ts",
    "_fs/_fs_common.ts",
    "_fs/_fs_constants.ts",
    "_fs/_fs_copy.ts",
    "_fs/_fs_dir.ts",
    "_fs/_fs_dirent.ts",
    "_fs/_fs_exists.ts",
    "_fs/_fs_fdatasync.ts",
    "_fs/_fs_fstat.ts",
    "_fs/_fs_fsync.ts",
    "_fs/_fs_ftruncate.ts",
    "_fs/_fs_futimes.ts",
    "_fs/_fs_link.ts",
    "_fs/_fs_lstat.ts",
    "_fs/_fs_mkdir.ts",
    "_fs/_fs_mkdtemp.ts",
    "_fs/_fs_open.ts",
    "_fs/_fs_opendir.ts",
    "_fs/_fs_read.ts",
    "_fs/_fs_readdir.ts",
    "_fs/_fs_readFile.ts",
    "_fs/_fs_readlink.ts",
    "_fs/_fs_realpath.ts",
    "_fs/_fs_rename.ts",
    "_fs/_fs_rm.ts",
    "_fs/_fs_rmdir.ts",
    "_fs/_fs_stat.ts",
    "_fs/_fs_symlink.ts",
    "_fs/_fs_truncate.ts",
    "_fs/_fs_unlink.ts",
    "_fs/_fs_utimes.ts",
    "_fs/_fs_watch.ts",
    "_fs/_fs_write.mjs",
    "_fs/_fs_writeFile.ts",
    "_fs/_fs_writev.mjs",
    "_http_agent.mjs",
    "_http_common.ts",
    "_http_outgoing.ts",
    "_next_tick.ts",
    "_process/exiting.ts",
    "_process/process.ts",
    "_process/streams.mjs",
    "_readline.mjs",
    "_stream.mjs",
    "_tls_common.ts",
    "_tls_wrap.ts",
    "_util/_util_callbackify.ts",
    "_util/asserts.ts",
    "_util/async.ts",
    "_util/os.ts",
    "_util/std_asserts.ts",
    "_util/std_fmt_colors.ts",
    "_util/std_testing_diff.ts",
    "_utils.ts",
    "_zlib_binding.mjs",
    "_zlib.mjs",
    "assert.ts",
    "assert/strict.ts",
    "assertion_error.ts",
    "async_hooks.ts",
    "buffer.ts",
    "child_process.ts",
    "cluster.ts",
    "console.ts",
    "constants.ts",
    "crypto.ts",
    "dgram.ts",
    "diagnostics_channel.ts",
    "dns.ts",
    "dns/promises.ts",
    "domain.ts",
    "events.ts",
    "fs.ts",
    "fs/promises.ts",
    "http.ts",
    "http2.ts",
    "https.ts",
    "inspector.ts",
    "internal_binding/_libuv_winerror.ts",
    "internal_binding/_listen.ts",
    "internal_binding/_node.ts",
    "internal_binding/_timingSafeEqual.ts",
    "internal_binding/_utils.ts",
    "internal_binding/ares.ts",
    "internal_binding/async_wrap.ts",
    "internal_binding/buffer.ts",
    "internal_binding/cares_wrap.ts",
    "internal_binding/connection_wrap.ts",
    "internal_binding/constants.ts",
    "internal_binding/crypto.ts",
    "internal_binding/handle_wrap.ts",
    "internal_binding/mod.ts",
    "internal_binding/node_file.ts",
    "internal_binding/node_options.ts",
    "internal_binding/pipe_wrap.ts",
    "internal_binding/stream_wrap.ts",
    "internal_binding/string_decoder.ts",
    "internal_binding/symbols.ts",
    "internal_binding/tcp_wrap.ts",
    "internal_binding/types.ts",
    "internal_binding/udp_wrap.ts",
    "internal_binding/util.ts",
    "internal_binding/uv.ts",
    "internal/assert.mjs",
    "internal/async_hooks.ts",
    "internal/buffer.mjs",
    "internal/child_process.ts",
    "internal/cli_table.ts",
    "internal/console/constructor.mjs",
    "internal/constants.ts",
    "internal/crypto/_keys.ts",
    "internal/crypto/_randomBytes.ts",
    "internal/crypto/_randomFill.ts",
    "internal/crypto/_randomInt.ts",
    "internal/crypto/certificate.ts",
    "internal/crypto/cipher.ts",
    "internal/crypto/constants.ts",
    "internal/crypto/diffiehellman.ts",
    "internal/crypto/hash.ts",
    "internal/crypto/hkdf.ts",
    "internal/crypto/keygen.ts",
    "internal/crypto/keys.ts",
    "internal/crypto/pbkdf2.ts",
    "internal/crypto/random.ts",
    "internal/crypto/scrypt.ts",
    "internal/crypto/sig.ts",
    "internal/crypto/util.ts",
    "internal/crypto/x509.ts",
    "internal/dgram.ts",
    "internal/dns/promises.ts",
    "internal/dns/utils.ts",
    "internal/dtrace.ts",
    "internal/error_codes.ts",
    "internal/errors.ts",
    "internal/event_target.mjs",
    "internal/fixed_queue.ts",
    "internal/fs/streams.mjs",
    "internal/fs/utils.mjs",
    "internal/fs/handle.ts",
    "internal/hide_stack_frames.ts",
    "internal/http.ts",
    "internal/idna.ts",
    "internal/net.ts",
    "internal/normalize_encoding.mjs",
    "internal/options.ts",
    "internal/primordials.mjs",
    "internal/process/per_thread.mjs",
    "internal/querystring.ts",
    "internal/readline/callbacks.mjs",
    "internal/readline/emitKeypressEvents.mjs",
    "internal/readline/interface.mjs",
    "internal/readline/promises.mjs",
    "internal/readline/symbols.mjs",
    "internal/readline/utils.mjs",
    "internal/stream_base_commons.ts",
    "internal/streams/add-abort-signal.mjs",
    "internal/streams/buffer_list.mjs",
    "internal/streams/destroy.mjs",
    "internal/streams/duplex.mjs",
    "internal/streams/end-of-stream.mjs",
    "internal/streams/lazy_transform.mjs",
    "internal/streams/passthrough.mjs",
    "internal/streams/readable.mjs",
    "internal/streams/state.mjs",
    "internal/streams/transform.mjs",
    "internal/streams/utils.mjs",
    "internal/streams/writable.mjs",
    "internal/test/binding.ts",
    "internal/timers.mjs",
    "internal/url.ts",
    "internal/util.mjs",
    "internal/util/comparisons.ts",
    "internal/util/debuglog.ts",
    "internal/util/inspect.mjs",
    "internal/util/types.ts",
    "internal/validators.mjs",
    "net.ts",
    "os.ts",
    "path.ts",
    "path/_constants.ts",
    "path/_interface.ts",
    "path/_util.ts",
    "path/_posix.ts",
    "path/_win32.ts",
    "path/common.ts",
    "path/mod.ts",
    "path/posix.ts",
    "path/separator.ts",
    "path/win32.ts",
    "perf_hooks.ts",
    "process.ts",
    "punycode.ts",
    "querystring.ts",
    "readline.ts",
    "readline/promises.ts",
    "repl.ts",
    "stream.ts",
    "stream/consumers.mjs",
    "stream/promises.mjs",
    "stream/web.ts",
    "string_decoder.ts",
    "sys.ts",
    "timers.ts",
    "timers/promises.ts",
    "tls.ts",
    "tty.ts",
    "url.ts",
    "util.ts",
    "util/types.ts",
    "v8.ts",
    "vm.ts",
    "wasi.ts",
    "worker_threads.ts",
    "zlib.ts",
  ],
  options = {
    maybe_npm_resolver: Option<NpmResolverRc>,
    fs: deno_fs::FileSystemRc,
  },
  state = |state, options| {
    let fs = options.fs;
    state.put(fs.clone());
    if let Some(npm_resolver) = options.maybe_npm_resolver {
      state.put(npm_resolver.clone());
      state.put(Rc::new(NodeResolver::new(
        fs,
        npm_resolver,
      )))
    }
  },
);

pub fn initialize_runtime(
  js_runtime: &mut JsRuntime,
  uses_local_node_modules_dir: bool,
  maybe_binary_command_name: Option<&str>,
) -> Result<(), AnyError> {
  let argv0 = if let Some(binary_command_name) = maybe_binary_command_name {
    serde_json::to_string(binary_command_name)?
  } else {
    "undefined".to_string()
  };
  let source_code = format!(
    r#"(function loadBuiltinNodeModules(nodeGlobalThisName, usesLocalNodeModulesDir, argv0) {{
      Deno[Deno.internal].node.initialize(
        nodeGlobalThisName,
        usesLocalNodeModulesDir,
        argv0
      );
      // Make the nodeGlobalThisName unconfigurable here.
      Object.defineProperty(globalThis, nodeGlobalThisName, {{ configurable: false }});
    }})('{}', {}, {});"#,
    NODE_GLOBAL_THIS_NAME, uses_local_node_modules_dir, argv0
  );

  js_runtime.execute_script(located_script_name!(), source_code.into())?;
  Ok(())
}

pub fn load_cjs_module(
  js_runtime: &mut JsRuntime,
  module: &str,
  main: bool,
  inspect_brk: bool,
) -> Result<(), AnyError> {
  fn escape_for_single_quote_string(text: &str) -> String {
    text.replace('\\', r"\\").replace('\'', r"\'")
  }

  let source_code = format!(
    r#"(function loadCjsModule(moduleName, isMain, inspectBrk) {{
      Deno[Deno.internal].node.loadCjsModule(moduleName, isMain, inspectBrk);
    }})('{module}', {main}, {inspect_brk});"#,
    main = main,
    module = escape_for_single_quote_string(module),
    inspect_brk = inspect_brk,
  )
  .into();

  js_runtime.execute_script(located_script_name!(), source_code)?;
  Ok(())
}
