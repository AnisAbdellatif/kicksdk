// The C ABI of crates/kicksdk-ffi. Everything above this file speaks JSON
// through these six functions.
@DefaultAsset('package:kicksdk/kicksdk_ffi')
library;

import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

/// The ABI this package was written against.
const abiVersion = 1;

@Native<Uint32 Function()>(symbol: 'kicksdk_abi_version')
external int _abiVersion();

@Native<Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>)>(symbol: 'kicksdk_call')
external Pointer<Utf8> _call(Pointer<Utf8> name, Pointer<Utf8> args);

@Native<Pointer<Void> Function(Pointer<Utf8>)>(symbol: 'kicksdk_session_new')
external Pointer<Void> _sessionNew(Pointer<Utf8> channels);

@Native<Pointer<Utf8> Function(Pointer<Void>, Pointer<Utf8>, Pointer<Utf8>)>(symbol: 'kicksdk_session_call')
external Pointer<Utf8> _sessionCall(Pointer<Void> session, Pointer<Utf8> method, Pointer<Utf8> args);

@Native<Void Function(Pointer<Void>)>(symbol: 'kicksdk_session_free')
external void _sessionFree(Pointer<Void> session);

@Native<Void Function(Pointer<Utf8>)>(symbol: 'kicksdk_free_string')
external void _freeString(Pointer<Utf8> s);

/// Kick answered with an error, or a call could not be made.
class KickException implements Exception {
  /// `status`, `shape`, `json`, `invalid`, `webhook`, `call` or `panic`.
  final String kind;

  /// Kick's HTTP status, for `status`.
  final int? status;
  final String message;

  KickException(this.kind, this.status, this.message);

  @override
  String toString() => 'KickException($kind${status == null ? '' : ' $status'}): $message';
}

var _checked = false;

void _checkAbi() {
  if (_checked) return;
  final found = _abiVersion();
  if (found != abiVersion) {
    throw StateError('kicksdk: native library has ABI $found, this package wants $abiVersion');
  }
  _checked = true;
}

Object? _open(Pointer<Utf8> out) {
  try {
    final envelope = jsonDecode(out.toDartString()) as Map<String, dynamic>;
    if (envelope.containsKey('ok')) return envelope['ok'];
    final e = envelope['error'] as Map<String, dynamic>;
    throw KickException(e['kind'] as String, e['status'] as int?, e['message'] as String);
  } finally {
    _freeString(out);
  }
}

/// Calls one core function by name (see `kicksdk::dispatch`).
Object? call(String name, [Map<String, Object?> args = const {}]) {
  _checkAbi();
  final n = name.toNativeUtf8();
  final a = jsonEncode(args).toNativeUtf8();
  try {
    return _open(_call(n, a));
  } finally {
    malloc.free(n);
    malloc.free(a);
  }
}

final _free = Native.addressOf<NativeFunction<Void Function(Pointer<Void>)>>(_sessionFree);

/// A native session, freed when this object is collected or [close]d.
class NativeSession implements Finalizable {
  static final _finalizer = NativeFinalizer(_free);
  Pointer<Void> _ptr;

  NativeSession(List<String> channels) : _ptr = nullptr {
    _checkAbi();
    final c = jsonEncode(channels).toNativeUtf8();
    try {
      _ptr = _sessionNew(c);
    } finally {
      malloc.free(c);
    }
    if (_ptr == nullptr) throw ArgumentError.value(channels, 'channels');
    _finalizer.attach(this, _ptr, detach: this);
  }

  Object? call(String method, [Map<String, Object?> args = const {}]) {
    if (_ptr == nullptr) throw StateError('kicksdk: session closed');
    final m = method.toNativeUtf8();
    final a = jsonEncode(args).toNativeUtf8();
    try {
      return _open(_sessionCall(_ptr, m, a));
    } finally {
      malloc.free(m);
      malloc.free(a);
    }
  }

  void close() {
    if (_ptr == nullptr) return;
    _finalizer.detach(this);
    _sessionFree(_ptr);
    _ptr = nullptr;
  }
}
