"""Generator-only checks: no browser, runtime, credentials or device emulation."""

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "lab/scripts/15-ui-layout-fixture.py"
spec = importlib.util.spec_from_file_location("layout_fixture", SCRIPT)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class LayoutFixtureTests(unittest.TestCase):
    def test_source_is_inert_and_css_is_exact(self):
        css = (ROOT / "src/ui/assets/style.css").read_text()
        html, actual = fixture.fixture_sources((ROOT / "src/ui/assets/index.html").read_text(), css)
        self.assertEqual(actual, css)
        self.assertNotIn("<script", html)
        self.assertNotIn("<link", html)
        self.assertNotIn("href=", html)
        self.assertIn("script-src &#x27;none&#x27;", html)
        self.assertIn('id="composer"', html)

    def test_external_active_markup_removed(self):
        source = '''<html><head><base href="https://invalid/">
        <meta http-equiv="refresh" content="0;url=https://invalid/">
        <script src="https://invalid/"></script><link href="https://invalid/">
        </head><body onload="alert(1)"><iframe src="https://invalid/"></iframe>
        <img src="https://invalid/"><form action="https://invalid/">
        <button onclick="alert(1)" formaction="https://invalid/">Alex</button>
        </form><style>body{background:url(https://invalid/)}</style></body></html>'''
        html, _ = fixture.fixture_sources(source, "body{margin:0}")
        for forbidden in ["https://", "onload", "onclick", "formaction", "<style", "<script", "<iframe", "<img", "<link", "<base"]:
            self.assertNotIn(forbidden, html)
        self.assertIn("Alex", html)

    def test_fetching_css_fails_closed(self):
        for css in ['a{background:url(x)}', '@import "x";', r'a{background:u\72l(x)}',
                    'a{background:u/**/rl(x)}', '@font-face{src:local(x)}']:
            with self.subTest(css=css), self.assertRaises(ValueError):
                fixture.fixture_sources("<html><head></head></html>", css)

    def test_guard_sandbox_and_case_matrix(self):
        payload = fixture.build_payload("owned-fake-target", "<html><head></head></html>", "body{margin:0}")
        js = payload["expression"]
        self.assertLess(js.index("location.href !== 'about:blank'"), js.index("document.open()"))
        self.assertIn("document.head.childElementCount", js)
        self.assertIn("window.opener !== null", js)
        self.assertIn("frame.setAttribute('sandbox', 'allow-same-origin')", js)
        self.assertNotIn("'allow-scripts'", js)
        self.assertNotIn("fetch(", js)
        self.assertNotIn("sessionStorage", js)
        self.assertIn("[false,true]", js)
        self.assertIn("['expanded','compact','auto','drawer-open']", js)
        self.assertEqual(fixture.VIEWPORTS, [(320,568),(390,844),(844,390),(1280,720),(1920,1080)])
        for invariant in ["composerInViewport", "chatSpace", "fixedToastLayout", "horizontalOverflow"]:
            self.assertIn(invariant, js)
        self.assertEqual(payload["target_id"], "owned-fake-target")
        self.assertTrue(payload["await_promise"])

    def test_five_modal_cases_are_separate_and_use_native_dialog(self):
        js = fixture.build_payload("owned-fake-target", "<head></head>", "")["expression"]
        modal = js[js.index("  // Separate case type:"):]
        self.assertIn("for (const [width,height] of config.viewports)", modal)
        self.assertIn("type:'settings-modal'", modal)
        self.assertIn("checksLabel:report.caseGroups.modal.checksLabel", modal)
        self.assertIn("$('dialog').showModal()", modal)
        self.assertIn("$('dialog').matches(':modal')", modal)
        self.assertIn("n<24", modal)
        self.assertIn(".repeat(32)", modal)
        self.assertIn("frame.setAttribute('sandbox','allow-same-origin')", modal)
        self.assertNotIn("allow-scripts", modal)
        self.assertNotIn("allow-modals", modal)
        self.assertNotIn(".setAttribute('open'", modal)
        self.assertIn("style.textContent = config.css", modal)
        self.assertIn("result.error = {phase,name:error.name,message:error.message}", modal)
        for invariant in ["headerInViewport", "closeInViewport", "saveFooterInViewport", "saveButtonInViewport",
                          "bodyOverflowBounded", "bodyCanScroll", "longErrorBounded", "longErrorCanScroll",
                          "controlsStableAfterScroll"]:
            self.assertIn(invariant, modal)
        self.assertIn("report.modalCases.push(result)", modal)
        self.assertIn("report.totalCases = report.cases.length + report.modalCases.length", modal)
        self.assertIn("report.cases.every(item => item.pass) && report.modalCases.every(item => item.pass)", modal)
        # Workspace checks retain the accepted thresholds, not modal thresholds.
        workspace = js[:js.index("  // Separate case type:")]
        self.assertIn("chatSpace: visibleChat >= (height <= 500 ? 90 : 120)", workspace)
        self.assertIn("toastDoesNotCoverComposer: !intersects(before,boxes['#composer'])", workspace)

    @unittest.skipUnless(shutil.which("node"), "optional JS syntax checker unavailable")
    def test_generated_javascript_syntax_without_execution(self):
        payload = fixture.build_payload("owned-fake-target",
                                        (ROOT / "src/ui/assets/index.html").read_text(),
                                        (ROOT / "src/ui/assets/style.css").read_text())
        subprocess.run(["node", "--check", "-"], input=payload["expression"],
                       text=True, capture_output=True, check=True)

    @unittest.skipUnless(shutil.which("node"), "optional JS guard checker unavailable")
    def test_rerun_guard_accepts_only_intact_completed_fixture(self):
        # Run only the guard in a tiny VM model, never a browser or layout engine.
        guards = []
        for rerun in [False, True]:
            js = fixture.build_payload("owned-fake-target", "<head></head>", "", rerun)["expression"]
            guards.append(js[:js.index("  window.__kkStaticFixture = true;")] + "return true; })()")
        checker = r"""
const vm = require('node:vm'), assert = require('node:assert/strict');
const guards = GUARDS, label = LABEL;
function context(marked=false,withModals=false) {
  const report = {label,cases:Array(34).fill({})};
  if (withModals) { report.modalCases=Array(5).fill({type:'settings-modal'}); report.totalCases=39; }
  const window = {opener:null}; window.top = window;
  const document = {URL:'about:blank',title:marked ? 'K&K STATIC layout fixture' : '',
    head:{childElementCount:marked ? 2 : 0,children:marked ? [{tagName:'META'},{tagName:'TITLE'}] : []},
    body:{childElementCount:marked ? 2 : 0,textContent:marked ? label : '',children:marked ? [
      {tagName:'H1',textContent:label},{tagName:'PRE',textContent:JSON.stringify(report,null,2)}] : []}};
  if (marked) { window.__kkStaticFixture=true; window.__kkStaticFixtureReport=report; }
  return {window,document,location:{href:'about:blank'}};
}
(async () => {
  assert.equal(await vm.runInNewContext(guards[0],context()),true);
  assert.equal(await vm.runInNewContext(guards[1],context(true)),true);
  assert.equal(await vm.runInNewContext(guards[1],context(true,true)),true);
  await assert.rejects(vm.runInNewContext(guards[0],context(true)),/Refusing/);
  await assert.rejects(vm.runInNewContext(guards[1],context()),/Refusing/);
  const mutations = [
    c=>{c.location.href='http://127.0.0.1:9000/';}, c=>{c.document.URL='https://invalid/';},
    c=>{c.window.top={};}, c=>{c.window.opener={};}, c=>{c.window.__kkStaticFixture=false;},
    c=>{c.document.title='Operator page';}, c=>{c.document.body.children[1].textContent='changed';},
    c=>{c.document.body.childElementCount=3;}, c=>{c.document.head.children[0].tagName='SCRIPT';},
    c=>{c.window.__kkStaticFixtureReport.cases=[];}
  ];
  for (const mutate of mutations) { const c=context(true); mutate(c);
    await assert.rejects(vm.runInNewContext(guards[1],c),/Refusing/); }
})().catch(error=>{console.error(error);process.exitCode=1;});
"""
        checker = checker.replace("GUARDS", json.dumps(guards), 1).replace("LABEL", json.dumps(fixture.LABEL), 1)
        subprocess.run(["node", "-"], input=checker, text=True, capture_output=True, check=True)

    def test_missing_head_and_oversized_sources_rejected(self):
        with self.assertRaises(ValueError):
            fixture.fixture_sources("<body>Alex</body>", "")
        with self.assertRaises(ValueError):
            fixture.fixture_sources("<head></head>", " " * (128 * 1024 + 1))

    def test_cli_rejects_symlink_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            link = Path(directory) / "link"
            link.symlink_to(directory, target_is_directory=True)
            result = subprocess.run(["uv", "run", "--script", str(SCRIPT), "--target", "owned-fake-target",
                                     "--payload-directory", str(link)], text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("non-symlink", result.stderr)

    def test_cli_generates_only_private_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(["uv", "run", "--script", str(SCRIPT), "--target", "owned-fake-target",
                                     "--payload-directory", directory], text=True, capture_output=True, check=True)
            summary = json.loads(result.stdout)
            path = Path(summary["payload"])
            self.assertEqual(path.parent, Path(directory))
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertFalse(summary["executed"])
            self.assertIn("NOT actual interactive app", summary["label"])
            self.assertEqual(json.loads(path.read_text())["target_id"], "owned-fake-target")

    def test_shared_output_parent_gets_private_child(self):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory).chmod(0o770)
            result = subprocess.run(["uv", "run", "--script", str(SCRIPT), "--target", "owned-fake-target",
                                     "--rerun-owned-fixture", "--payload-directory", directory],
                                    text=True, capture_output=True, check=True)
            path = Path(json.loads(result.stdout)["payload"])
            self.assertEqual(path.parent.parent, Path(directory))
            self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertIn('"rerun": true', json.loads(path.read_text())["expression"])


if __name__ == "__main__":
    unittest.main()
