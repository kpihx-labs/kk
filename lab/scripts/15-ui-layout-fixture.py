#!/usr/bin/env -S uv run --script
"""Generate ONLY a browser-proxy JSON request for an explicitly owned blank tab.

STATIC fixture, not the interactive application or native/mobile qualification.
No browser command is executed. Main must inspect/approve the target and request.
"""

import argparse
import hashlib
from html import escape
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import stat
import tempfile


ROOT = Path(__file__).resolve().parents[2]
VIEWPORTS = [(320, 568), (390, 844), (844, 390), (1280, 720), (1920, 1080)]
LABEL = "STATIC rendered fixture ONLY; NOT actual interactive app or native/mobile proof"
CSP = ("default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; "
       "img-src 'none'; font-src 'none'; connect-src 'none'; media-src 'none'; "
       "object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'")


class StaticHTML(HTMLParser):
    """Retain layout markup, never source resource/navigation/active attributes."""

    tags = set("html head title body div span h1 h2 h3 p br aside main header nav "
               "section button strong small code details summary label input textarea "
               "form dialog a article time ul li select option".split())
    void = {"br", "input"}
    attrs = {"id", "class", "lang", "hidden", "open", "type", "rows", "maxlength",
             "placeholder", "title", "role", "disabled", "checked", "name", "value"}

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self.blocked = []

    def handle_starttag(self, tag, attrs):
        if self.blocked:
            if tag == self.blocked[-1]:
                self.blocked.append(tag)
            return
        if tag in {"script", "style", "iframe", "object", "template", "svg", "math"}:
            self.blocked.append(tag)
            return
        if tag not in self.tags:
            return
        safe = [(key, value) for key, value in attrs
                if key in self.attrs or key.startswith(("aria-", "data-"))]
        self.parts.append("<" + tag + "".join(
            " " + key + ("" if value is None else '="' + escape(value, quote=True) + '"')
            for key, value in safe) + ">")
        if tag == "head":
            self.parts.append('<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="'
                              + escape(CSP, quote=True) + '">')

    def handle_endtag(self, tag):
        if self.blocked:
            if tag == self.blocked[-1]:
                self.blocked.pop()
            return
        if tag in self.tags and tag not in self.void:
            self.parts.append("</" + tag + ">")

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if tag not in self.void:
            self.handle_endtag(tag)

    def handle_data(self, data):
        if not self.blocked:
            self.parts.append(escape(data))


def fixture_sources(html, css):
    if len(html.encode()) > 128 * 1024 or len(css.encode()) > 128 * 1024:
        raise ValueError("source exceeds static fixture bound")
    # Fail closed rather than rewrite CSS: the fixture must use the EXACT CSS.
    # Disallow escapes/comments that could conceal fetching tokens as well.
    if re.search(r"url\s*\(|@import|@font-face|image-set\s*\(|/\*|\\|</style", css, re.I):
        # Ordinary comments are harmless; strip them for inspection, not injection.
        inspected = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
        if re.search(r"url\s*\(|@import|@font-face|image-set\s*\(|\\|</style", inspected, re.I):
            raise ValueError("CSS with fetching/escaped tokens is not accepted")
    parser = StaticHTML()
    parser.feed(html)
    parser.close()
    sanitized = "<!doctype html>" + "".join(parser.parts)
    if '<meta http-equiv="Content-Security-Policy"' not in sanitized:
        raise ValueError("fixture source needs an explicit head")
    return sanitized, css


EXPRESSION = r"""(async () => {
  'use strict';
  // FIRST operation: reject real pages and child frames, even in rerun mode.
  if (window !== window.top || location.href !== 'about:blank' || document.URL !== 'about:blank' ||
      !document.body || !document.head || window.opener !== null) {
    throw new Error('Refusing: need an explicitly owned top-level about:blank tab without opener');
  }
  const config = CONFIG;
  const fresh = !window.__kkStaticFixture && !document.body.childElementCount &&
    !document.body.textContent.trim() && !document.head.childElementCount;
  const previous = window.__kkStaticFixture === true &&
    window.__kkStaticFixtureReport?.label === config.label &&
    window.__kkStaticFixtureReport?.cases?.length === 34 &&
    document.title === 'K&K STATIC layout fixture' &&
    document.head.childElementCount === 2 &&
    Array.from(document.head.children).map(node => node.tagName).join(',') === 'META,TITLE' &&
    document.body.childElementCount === 2 &&
    document.body.children[0].tagName === 'H1' && document.body.children[0].textContent === config.label &&
    document.body.children[1].tagName === 'PRE' &&
    document.body.children[1].textContent === JSON.stringify(window.__kkStaticFixtureReport,null,2);
  if (!(config.rerun ? previous : fresh)) {
    throw new Error('Refusing: default needs fresh blank; --rerun-owned-fixture needs intact completed marked fixture');
  }
  window.__kkStaticFixture = true;
  // No application document is written. Only this already-guarded blank is owned.
  document.open();
  document.write('<!doctype html><html><head><meta charset="utf-8"><title>K&K STATIC layout fixture</title></head><body></body></html>');
  document.close();
  const heading = document.createElement('h1'); heading.textContent = config.label;
  document.body.append(heading);
  const report = {label: config.label, sources: config.hashes, cases: [], modalCases: [],
    caseGroups: {workspace: {type:'workspace-layout',checksLabel:'Original 34 workspace layout invariants'},
      modal: {type:'settings-modal',checksLabel:'Static long Settings + conflict error containment and scroll'}},
    limits: {scripts: false, network: false, appRuntime: false, nativeMobile: false,
      hoverFocusAndInput: 'not exercised', viewport: 'iframe CSS pixels; not device emulation'}};
  const deadline = performance.now() + 30000;
  const waitFrame = () => new Promise((resolve,reject) => {
    const id = requestAnimationFrame(() => { clearTimeout(timer); resolve(); });
    const timer = setTimeout(() => { cancelAnimationFrame(id); reject(new Error('fixture paint timeout; keep owned tab foreground')); },1500);
  });
  const rect = el => {
    const r = el.getBoundingClientRect();
    return Object.fromEntries(['x','y','width','height','top','right','bottom','left'].map(k => [k, Math.round(r[k]*100)/100]));
  };
  const inside = (r,w,h) => r.width > 0 && r.height > 0 && r.left >= -1 && r.top >= -1 && r.right <= w+1 && r.bottom <= h+1;
  const intersects = (a,b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
  for (const [width,height] of config.viewports) for (const expanded of [false,true])
    for (const variant of ['expanded','compact','auto','drawer-open']) {
      if (variant === 'drawer-open' && width > 700) continue;
      if (performance.now() > deadline) throw new Error('static fixture exceeded 30-second work budget');
      const frame = document.createElement('iframe');
      frame.setAttribute('sandbox', 'allow-same-origin'); // Deliberately NO allow-scripts.
      frame.setAttribute('referrerpolicy','no-referrer');
      frame.title = `STATIC ${width}x${height} ${variant} ${expanded ? 'expanded' : 'collapsed'}`;
      frame.style.cssText = `display:block;border:0;width:${width}px;height:${height}px;pointer-events:none`;
      try {
        await new Promise((resolve,reject) => {
          const timer = setTimeout(() => reject(new Error('fixture load timeout')), 2500);
          frame.onload = () => { clearTimeout(timer); resolve(); };
          frame.srcdoc = config.html;
          document.body.append(frame);
        });
        const d = frame.contentDocument, $ = id => {
          const el = d.getElementById(id); if (!el) throw new Error('Missing static selector: '+id); return el;
        };
        const style = d.createElement('style'); style.textContent = config.css; d.head.append(style);
        const el = (tag,cls,text) => { const node = d.createElement(tag); node.className = cls; node.textContent = text; return node; };
        $('gate').hidden = true; $('welcome').hidden = true;
        $('workspace').hidden = false; $('room-view').hidden = false;
        $('workspace').dataset.sidebar = variant === 'drawer-open' ? 'expanded' : variant;
        $('sidebar').classList.toggle('open', variant === 'drawer-open');
        $('sidebar-backdrop').hidden = variant !== 'drawer-open';
        $('profile-name').textContent = 'Alex'; $('identity').textContent = 'STATIC-PUBLIC-FIXTURE';
        $('version').textContent = 'static fixture'; $('room-title').textContent = 'Study group';
        $('room-state').textContent = 'STATIC / ONLINE'; $('room-id').textContent = 'static-room-no-credentials';
        $('quorum').textContent = '2 members / 2 controllers';
        for (const title of ['Study group','Weekend plans','Reading circle']) {
          const button = el('button','room-link'+(title === 'Study group' ? ' selected' : ''),'');
          const text = el('span','',''); text.append(el('strong','',title),el('small','','static / online'));
          button.append(el('span','room-icon','#'),text); $('rooms').append(button);
        }
        for (const name of ['Alex','Morgan']) {
          const peer = el('div','peer',''), text = el('span','','');
          text.append(el('strong','',name),el('small','','Voting controller'));
          peer.append(el('span','peer-avatar',name.slice(0,2).toUpperCase()),text); $('peers').append(peer);
        }
        $('files').append(el('div','file-card','Study notes.txt / 2 KiB / STATIC'));
        $('messages').replaceChildren();
        for (let n=0;n<8;n++) {
          const message = el('article','message'+(n%2 ? ' outgoing' : ''),'');
          message.append(el('div','message-meta',n%2 ? 'Alex / STATIC' : 'Morgan / STATIC'),
            el('div','message-body','Study group notes: ordinary fixture text, not a real conversation.'));
          $('messages').append(message);
        }
        $('text').value = 'A static draft from Alex.';
        for (const panel of d.querySelectorAll('.people-strip, #files-panel, .technical-details')) panel.open = expanded;
        // Actions are measured open separately from the content details modes.
        $('action-center').open = true; $('action-count').textContent = '1';
        $('operations').append(el('div','operation','Static fixture: room ready.'));
        $('toast').textContent = 'Static fixture notice'; $('toast').hidden = false;
        await waitFrame(); await waitFrame();
        const boxes = {};
        for (const selector of ['#workspace','#sidebar','.main','.topbar','#room-view','.room-content',
          '.room-heading','.people-strip','#files-panel','#messages','#composer','#text','#send','#operations','#toast']) {
          const node = d.querySelector(selector); if (!node) throw new Error('Missing selector: '+selector);
          boxes[selector] = rect(node);
        }
        const scroll = d.querySelector('.room-content'), before = boxes['#toast'];
        scroll.scrollTop = scroll.scrollHeight;
        await waitFrame();
        const after = rect($('toast'));
        const offenders = Array.from(d.body.querySelectorAll('*')).filter(node => {
          if (!node.getClientRects().length) return false;
          const r = node.getBoundingClientRect(); return r.left < -1 || r.right > width+1;
        }).slice(0,20).map(node => ({selector: node.id ? '#'+node.id : node.tagName.toLowerCase()+'.'+node.className, box: rect(node)}));
        const visibleChat = Math.max(0,Math.min(boxes['#messages'].bottom,boxes['.room-content'].bottom,height)
          - Math.max(boxes['#messages'].top,boxes['.room-content'].top,0));
        const toastStyle = frame.contentWindow.getComputedStyle($('toast'));
        const checks = {
          noDocumentHorizontalOverflow: Math.max(d.documentElement.scrollWidth,d.body.scrollWidth) <= width+1,
          noElementHorizontalOverflow: offenders.length === 0,
          composerInViewport: inside(boxes['#composer'],width,height),
          chatSpace: visibleChat >= (height <= 500 ? 90 : 120),
          fixedToastLayout: toastStyle.position === 'fixed' && inside(before,width,height) &&
            Math.abs(before.x-after.x) <= 1 && Math.abs(before.y-after.y) <= 1,
          toastDoesNotCoverComposer: !intersects(before,boxes['#composer']),
          operationsInViewport: inside(boxes['#operations'],width,height)
        };
        report.cases.push({width,height,details: expanded ? 'expanded' : 'collapsed',rail:variant,boxes,
          horizontalOverflow: {document: Math.max(d.documentElement.scrollWidth,d.body.scrollWidth)-width,offenders},
          chatSpace: {visiblePixels: visibleChat,minimum: height <= 500 ? 90 : 120},
          toast: {position:toastStyle.position,before,after},checks,pass:Object.values(checks).every(Boolean)});
      } finally { frame.remove(); }
    }
  // Separate case type: never relax or replace the original workspace checks.
  for (const [width,height] of config.viewports) {
    if (performance.now() > deadline) throw new Error('static fixture exceeded 30-second work budget');
    const frame = document.createElement('iframe');
    frame.setAttribute('sandbox','allow-same-origin');
    frame.setAttribute('referrerpolicy','no-referrer');
    frame.title = `STATIC Settings modal ${width}x${height}`;
    frame.style.cssText = `display:block;border:0;width:${width}px;height:${height}px;pointer-events:none`;
    let phase = 'iframe load';
    const result = {type:'settings-modal',checksLabel:report.caseGroups.modal.checksLabel,width,height,
      seed:{settingsFields:24,conflictSentences:32},checks:{},pass:false};
    try {
      await new Promise((resolve,reject) => {
        const timer = setTimeout(() => reject(new Error('modal fixture load timeout')),2500);
        frame.onload = () => { clearTimeout(timer); resolve(); };
        frame.srcdoc = config.html; document.body.append(frame);
      });
      phase = 'parent DOM seeding';
      const d = frame.contentDocument, $ = id => {
        const node = d.getElementById(id); if (!node) throw new Error('Missing modal selector: '+id); return node;
      };
      const style = d.createElement('style'); style.textContent = config.css; d.head.append(style);
      $('gate').hidden = true;
      $('dialog-title').textContent = 'Settings'; $('dialog-submit').textContent = 'Save settings';
      for (let n=0;n<24;n++) {
        const field = d.createElement('label'); field.className = 'field';
        const label = d.createElement('span'); label.textContent = `Static preference ${n+1}`;
        const input = d.createElement('input'); input.type = 'text';
        input.value = n === 0 ? 'Alex' : 'Ordinary Settings fixture value';
        field.append(label,input); $('dialog-content').append(field);
      }
      $('dialog-error').textContent = 'Settings changed in another workspace. Review the current values before saving again. '.repeat(32);
      phase = 'parent dialog.showModal()';
      // Native HTML dialog only. No script permission, CSP change or fallback.
      $('dialog').showModal();
      phase = 'modal measurements';
      await waitFrame(); await waitFrame();
      const selectors = ['#dialog','.dialog-heading','#dialog-close','#dialog-content','#dialog-error',
        '.dialog-footer','#dialog-submit'];
      const boxes = Object.fromEntries(selectors.map(selector => [selector,rect(d.querySelector(selector))]));
      const scrollMetrics = node => ({clientHeight:node.clientHeight,scrollHeight:node.scrollHeight,
        clientWidth:node.clientWidth,scrollWidth:node.scrollWidth,scrollTop:node.scrollTop,
        overflowY:frame.contentWindow.getComputedStyle(node).overflowY});
      const body = $('dialog-content'), error = $('dialog-error');
      // showModal may autofocus and scroll a field; reset to prove actual movement.
      body.scrollTop = 0; error.scrollTop = 0;
      const bodyBefore = scrollMetrics(body), errorBefore = scrollMetrics(error);
      body.scrollTop = body.scrollHeight; error.scrollTop = error.scrollHeight;
      await waitFrame();
      const bodyAfter = scrollMetrics(body), errorAfter = scrollMetrics(error);
      const after = Object.fromEntries(selectors.map(selector => [selector,rect(d.querySelector(selector))]));
      const contained = (inner,outer) => inner.width > 0 && inner.height > 0 &&
        inner.left >= outer.left-1 && inner.right <= outer.right+1 &&
        inner.top >= outer.top-1 && inner.bottom <= outer.bottom+1;
      const stable = selector => ['top','bottom','left','right'].every(key => Math.abs(boxes[selector][key]-after[selector][key]) <= 1);
      const checks = {
        nativeModalShown: $('dialog').open && $('dialog').matches(':modal'),
        dialogInViewport: inside(boxes['#dialog'],width,height),
        headerInViewport: inside(boxes['.dialog-heading'],width,height),
        closeInViewport: inside(boxes['#dialog-close'],width,height),
        saveFooterInViewport: inside(boxes['.dialog-footer'],width,height),
        saveButtonInViewport: inside(boxes['#dialog-submit'],width,height),
        headerAndFooterInsideDialog: ['.dialog-heading','#dialog-close','.dialog-footer','#dialog-submit']
          .every(selector => contained(boxes[selector],boxes['#dialog'])),
        bodyOverflowBounded: contained(boxes['#dialog-content'],boxes['#dialog']) &&
          boxes['#dialog-content'].top >= boxes['.dialog-heading'].bottom-1 &&
          boxes['#dialog-content'].bottom <= boxes['#dialog-error'].top+1 &&
          ['auto','scroll'].includes(bodyBefore.overflowY) && bodyBefore.clientHeight > 0 &&
          bodyBefore.scrollHeight > bodyBefore.clientHeight+1 && bodyBefore.scrollWidth <= bodyBefore.clientWidth+1,
        bodyCanScroll: bodyAfter.scrollTop > bodyBefore.scrollTop+1,
        longErrorBounded: contained(boxes['#dialog-error'],boxes['#dialog']) &&
          boxes['#dialog-error'].bottom <= boxes['.dialog-footer'].top+1 &&
          ['auto','scroll'].includes(errorBefore.overflowY) && errorBefore.clientHeight > 0 &&
          boxes['#dialog-error'].height <= height*0.18+1 &&
          errorBefore.scrollHeight > errorBefore.clientHeight+1 && errorBefore.scrollWidth <= errorBefore.clientWidth+1,
        longErrorCanScroll: errorAfter.scrollTop > errorBefore.scrollTop+1,
        controlsStableAfterScroll: ['.dialog-heading','#dialog-close','.dialog-footer','#dialog-submit'].every(stable)
      };
      Object.assign(result,{boxes,after,bodyScroll:{before:bodyBefore,after:bodyAfter},
        errorScroll:{before:errorBefore,after:errorAfter},checks,pass:Object.values(checks).every(Boolean)});
    } catch (error) {
      // Include exact sandbox/showModal failure instead of enabling scripts/modals.
      result.error = {phase,name:error.name,message:error.message};
      result.checks.modalFixtureCompleted = false;
    } finally { frame.remove(); }
    report.modalCases.push(result);
  }
  report.totalCases = report.cases.length + report.modalCases.length;
  report.pass = report.cases.every(item => item.pass) && report.modalCases.every(item => item.pass);
  window.__kkStaticFixtureReport = report;
  const output = document.createElement('pre'); output.textContent = JSON.stringify(report,null,2); document.body.append(output);
  return report;
})()"""


def build_payload(target, html, css, rerun=False):
    sanitized, exact_css = fixture_sources(html, css)
    config = {"html": sanitized, "css": exact_css, "viewports": VIEWPORTS, "label": LABEL, "rerun": rerun,
              "hashes": {"index.html": hashlib.sha256(html.encode()).hexdigest(),
                         "style.css": hashlib.sha256(css.encode()).hexdigest()}}
    return {"profile": "default", "target_id": target, "await_promise": True,
            "expression": EXPRESSION.replace("CONFIG", json.dumps(config), 1)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="Main's explicitly owned fresh about:blank target ID")
    parser.add_argument("--payload-directory", type=Path, default=Path("/tmp/opencode"))
    parser.add_argument("--rerun-owned-fixture", action="store_true",
                        help="Require an intact completed marked fixture on the same owned blank tab, never an app")
    args = parser.parse_args()
    directory = args.payload_directory
    metadata = directory.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
        parser.error("payload directory must be existing, owned and non-symlink")
    if metadata.st_mode & 0o022:
        # The shared /tmp/opencode workspace may be group writable. Do not
        # chmod it or relax payload privacy: create an exclusive 0700 child.
        directory = Path(tempfile.mkdtemp(prefix="kk-layout-private-", dir=directory))
    payload = build_payload(args.target, (ROOT / "src/ui/assets/index.html").read_text(),
                            (ROOT / "src/ui/assets/style.css").read_text(), args.rerun_owned_fixture)
    fd, filename = tempfile.mkstemp(prefix="kk-static-layout-", suffix=".json", dir=directory)
    with os.fdopen(fd, "w", encoding="utf-8") as output:
        json.dump(payload, output)
    print(json.dumps({"payload": filename, "label": LABEL, "executed": False,
                      "next_command": ["browser-proxy", "do", "page-evaluate", filename]}))


if __name__ == "__main__":
    main()
