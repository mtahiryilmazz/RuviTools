/*
 * GeTube v2.0 — kesit zamanı yardımcıları (content script + testler).
 * Kullanıcı girdisi ("1:23", "01:23:45", "83", "1:23.5") saniyeye çevrilir;
 * üst zaman sınırı kaldırılmıştır (manuel başlangıç ve bitiş serbesttir).
 */
(function (root) {
  'use strict';

  var MAX_RANGE_SEC = Infinity;
  var MIN_RANGE_SEC = 1;

  function isNum(v) {
    return typeof v === 'number' && isFinite(v);
  }

  /** "1:23" | "01:23:45" | "83" | "1:23.5" -> saniye (number) | null */
  function parseTime(input) {
    if (input === null || input === undefined) return null;
    if (isNum(input)) return input >= 0 ? round3(input) : null;

    var s = String(input).trim().replace(',', '.');
    if (!s) return null;
    if (s.length > 16) return null;

    var parts = s.split(':');
    if (parts.length < 1 || parts.length > 3) return null;

    var total = 0;
    for (var i = 0; i < parts.length; i++) {
      var p = parts[i].trim();
      if (!/^\d+(\.\d+)?$/.test(p)) return null;
      var v = Number(p);
      if (!isFinite(v) || v < 0) return null;
      if (parts.length === 3) {
        if (i > 0 && v >= 60) return null;
      } else if (parts.length === 2) {
        if (i === 1 && v >= 60) return null;
      }
      total = total * 60 + v;
    }
    if (!isFinite(total) || total < 0) return null;
    return round3(total);
  }

  function round3(v) {
    return Math.round(v * 1000) / 1000;
  }

  /** 83 -> "1:23", 5025 -> "1:23:45" */
  function formatClock(sec) {
    if (!isNum(sec) || sec < 0) return '';
    var total = Math.floor(sec);
    var h = Math.floor(total / 3600);
    var m = Math.floor((total % 3600) / 60);
    var s = total % 60;
    if (h > 0) return h + ':' + pad2(m) + ':' + pad2(s);
    return m + ':' + pad2(s);
  }

  /** 5025 -> "01:23:45" */
  function formatStamp(sec, withMs) {
    if (!isNum(sec) || sec < 0) return '';
    var ms = Math.round((sec - Math.floor(sec)) * 1000);
    var total = Math.floor(sec);
    var h = Math.floor(total / 3600);
    var m = Math.floor((total % 3600) / 60);
    var s = total % 60;
    var out = pad2(h) + ':' + pad2(m) + ':' + pad2(s);
    if (withMs) out += '.' + pad3(ms);
    return out;
  }

  /** Dosya adı etiketi: 83 -> "00-01-23" */
  function formatFileStamp(sec) {
    return formatStamp(sec, false).replace(/:/g, '-');
  }

  /** 82 -> "1dk 22sn" */
  function formatDuration(sec) {
    if (!isNum(sec) || sec < 0) return '';
    var total = Math.round(sec);
    var h = Math.floor(total / 3600);
    var m = Math.floor((total % 3600) / 60);
    var s = total % 60;
    var out = [];
    if (h) out.push(h + 'sa');
    if (m) out.push(m + 'dk');
    if (s || !out.length) out.push(s + 'sn');
    return out.join(' ');
  }

  function pad2(n) { return (n < 10 ? '0' : '') + n; }
  function pad3(n) { return (n < 10 ? '00' : n < 100 ? '0' : '') + n; }

  /**
   * Aralığı doğrular (üst süre sınırı yoktur; başlangıç ve bitiş el ile serbestçe belirlenir).
   * @returns {{ok:boolean, code?:string, startSec?:number, endSec?:number}}
   */
  function validateRange(startInput, endInput, fallbackSec) {
    var start = parseTime(startInput);
    if (start === null) return { ok: false, code: 'errFormat' };

    var end;
    var endText = (endInput === null || endInput === undefined) ? '' : String(endInput).trim();
    if (!endText) {
      if (isNum(fallbackSec) && fallbackSec > 0) {
        end = round3(start + fallbackSec);
      } else {
        return { ok: false, code: 'errFormat' };
      }
    } else {
      end = parseTime(endText);
      if (end === null) return { ok: false, code: 'errFormat' };
    }

    var span = round3(end - start);
    if (span <= 0) return { ok: false, code: 'errOrder' };
    if (span < MIN_RANGE_SEC) return { ok: false, code: 'errShort' };
    return { ok: true, startSec: start, endSec: end };
  }

  var api = {
    MAX_RANGE_SEC: MAX_RANGE_SEC,
    MIN_RANGE_SEC: MIN_RANGE_SEC,
    parseTime: parseTime,
    formatClock: formatClock,
    formatStamp: formatStamp,
    formatFileStamp: formatFileStamp,
    formatDuration: formatDuration,
    validateRange: validateRange
  };

  if (typeof module === 'object' && module.exports) module.exports = api;
  root.GeTubeClipTime = api;
  root.RuviTubeClipTime = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
