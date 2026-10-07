'use strict';
const byId = id => document.getElementById(id);
let session = null, csrf = null, authorization = null, layouts = null, busy = false;
const aidByKey = {Enter:125,F1:241,F2:242,F3:243,F4:244,F5:245,F6:246,F7:247,F8:248,F9:249,F10:122,F11:123,F12:124};
function status(message, failed = false) {
  byId('status').textContent = message;
  byId('status').classList.toggle('failed', failed);
}
function controls() {
  byId('connect').disabled = busy || !!session;
  byId('disconnect').disabled = busy || !session;
  byId('role').disabled = busy || !!session;
  document.querySelectorAll('#keys button').forEach(button => button.disabled = busy || !session);
}
async function request(path, method = 'GET', body, binary = false) {
  const headers = {'Authorization':authorization, 'X-CSRF-ZOSMF-HEADER':'true'};
  if (csrf) headers['X-CSRF-TOKEN'] = csrf;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const response = await fetch(path, {method, headers, body:body === undefined ? undefined : JSON.stringify(body), cache:'no-store'});
  if (!response.ok) throw new Error(`Request failed (${response.status}): ${await response.text()}`);
  if (binary) return new Uint8Array(await response.arrayBuffer());
  return response.status === 204 ? null : response.json();
}
function decodeFields(encoded) {
  const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
  const view = new DataView(bytes.buffer), fields = {}, decoder = new TextDecoder();
  let offset = 0;
  while (offset < bytes.length) {
    if (offset + 4 > bytes.length) throw new Error('Incomplete screen field.');
    const nameSize = view.getUint32(offset); offset += 4;
    if (offset + nameSize + 4 > bytes.length) throw new Error('Incomplete screen name.');
    const name = decoder.decode(bytes.slice(offset, offset + nameSize)); offset += nameSize;
    const valueSize = view.getUint32(offset); offset += 4;
    if (offset + valueSize > bytes.length) throw new Error('Incomplete screen value.');
    fields[name] = decoder.decode(bytes.slice(offset, offset + valueSize)).replace(/\0/g,' ');
    offset += valueSize;
  }
  return fields;
}
function protection(record) {
  const addresses = {};
  for (let at = 2; at + 4 < record.length; at++) {
    if (record[at] === 0x11 && record[at + 3] === 0x1d) {
      addresses[((record[at + 1] & 0x3f) << 8) | record[at + 2]] = !!(record[at + 4] & 0x20);
      at += 4;
    }
  }
  return addresses;
}
async function render(terminal) {
  const layout = layouts[terminal.mapset];
  if (!layout || layout.map !== terminal.map) throw new Error('Unknown application screen.');
  const fields = decodeFields(terminal.screen_base64);
  const protectedFields = protection(await request(`/mainframe-env/cics/v1/sessions/${session}/tn3270`, 'GET', undefined, true));
  const screen = byId('screen'); screen.replaceChildren();
  for (const field of layout.fields) {
    if (!field.position || !field.length) continue;
    const [row,column] = field.position;
    const value = field.name && Object.hasOwn(fields, field.name) ? fields[field.name] : (field.initial || '');
    const address = (row - 1) * terminal.columns + column - 1;
    const protectedField = Object.hasOwn(protectedFields, address) ? protectedFields[address] : field.protected;
    const element = document.createElement(field.name && !protectedField ? 'input' : 'span');
    element.className = `field${field.name ? '' : ' literal'}${field.name === 'ERRMSG' ? ' error' : ''}`;
    element.style.gridRow = String(row);
    element.style.gridColumn = `${column} / span ${Math.min(field.length, 81 - column)}`;
    if (element.tagName === 'INPUT') {
      element.name = field.name;
      element.setAttribute('aria-label', field.name);
      element.maxLength = field.length;
      element.type = field.secret ? 'password' : 'text';
      element.value = field.secret ? '' : value.trimEnd();
      element.autocomplete = 'off'; element.spellcheck = false;
    } else element.textContent = field.secret ? '' : value;
    screen.append(element);
  }
  byId('screen-name').textContent = terminal.mapset;
  byId('connection').textContent = 'Connected';
  screen.querySelector('input')?.focus();
}
async function perform(action) {
  if (busy) return;
  busy = true; controls();
  try { await action(); } catch (error) { status(error.message, true); }
  finally { busy = false; controls(); }
}
byId('connect').addEventListener('click', () => perform(async () => {
  status('Connecting…');
  layouts ??= await (await fetch('/carddemo/layouts')).json();
  const user = byId('role').value;
  authorization = 'Basic ' + btoa(`${user}:${user === 'WEBADM' ? 'admin-transport-password' : 'transport-password'}`);
  const launched = await request('/mainframe-env/cics/v1/sessions', 'POST', {transaction:'CC00'});
  session = launched.session; csrf = launched.csrf_token;
  await render(launched.terminal);
  status('Enter your CardDemo user ID and password.');
}));
async function send(aid) {
  if (!session) return;
  await perform(async () => {
    status('Working…');
    const fields = {};
    byId('screen').querySelectorAll('input').forEach(input => {
      fields[input.name] = input.value;
    });
    await request(`/mainframe-env/cics/v1/sessions/${session}/input`, 'PUT', {aid,fields});
    const terminal = await request(`/mainframe-env/cics/v1/sessions/${session}/resume`, 'POST');
    await render(terminal);
    status('Ready.');
  });
}
byId('terminal-form').addEventListener('submit', event => {event.preventDefault(); send(125);});
document.querySelectorAll('#keys button').forEach(button => button.addEventListener('click', () => send(Number(button.dataset.aid))));
document.addEventListener('keydown', event => {
  if (session && Object.hasOwn(aidByKey,event.key)) {event.preventDefault(); send(aidByKey[event.key]);}
});
byId('disconnect').addEventListener('click', () => perform(async () => {
  await request(`/mainframe-env/cics/v1/sessions/${session}`, 'DELETE');
  session = null; csrf = null; authorization = null;
  byId('screen').replaceChildren(); byId('screen-name').textContent = '';
  byId('connection').textContent = 'Disconnected'; status('Disconnected.');
}));
controls();
byId('role').addEventListener('change', () => { byId('demo-user').textContent = byId('role').value === 'WEBADM' ? 'ADMIN001 / PASSWORD' : 'USER0001 / PASSWORD'; });
