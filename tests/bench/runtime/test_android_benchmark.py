"""Regression boundaries for complete Android report retrieval and fixture scope."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'dev/scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bench = load('benchmark-android')
prepare = load('prepare-android-benchmark')


class AndroidBenchmarkTests(unittest.TestCase):
    def test_rejects_incomplete_samples_and_retains_timeout_evidence(self):
        sample = dict(kind='render', target='bench_baseline', mode='continuous', label='baseline', args=[])
        trace = 'KEINE_TRACE\t1\t1\t16\t2\t16.67\tbaseline\t0\tscript.shou\tanimation\t1\ttrue\t1920\t1080\tnone\n'
        log = 'resolved cursor Some(1)\nKEINE_RENDER_SAMPLE frames=1 average_ms=16\n' + trace
        self.assertEqual(bench.validate_sample(sample, log)['frames'], 1)
        for bad in (log + 'ERROR asset missing', log.replace('frames=1', 'frames=0'),
                    log.replace('average_ms=16', 'average_ms=nan'), log.replace('resolved cursor Some(1)', ''),
                    log.replace(trace, ''), log.replace('\tnone', ''), log.replace('\ttrue', '\tfalse'), log + 'KEINE_RENDER_SAMPLE frames=1\n'):
            with self.subTest(log=bad):
                with self.assertRaises(ValueError):
                    bench.validate_sample(sample, bad)
        class FakeAdb:
            def __init__(self): self.stops = 0
            def stop(self): self.stops += 1
            def private(self, *args, **kwargs): pass
            def shell(self, *args): return 'Status: ok'
            def context(self): return {'thermal': 'n/a'}
            def read(self, name, **kwargs): return 'partial engine log' if name == 'sample.txt' else ''
        fake = FakeAdb()
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'sample'
            with patch.object(bench.time, 'monotonic', side_effect=[0, 121]):
                with self.assertRaises(TimeoutError):
                    bench.run_sample(fake, sample, directory, 120)
            self.assertEqual((directory / 'sample.txt').read_text(), 'partial engine log')
            self.assertTrue((directory / 'after.json').is_file())
            self.assertEqual(fake.stops, 2)

    def test_derivation_preserves_nonvideo_workloads_and_separate_identity(self):
        source = ROOT / 'tests/fixtures/native-benchmark'
        original = {str(p.relative_to(source)): p.read_bytes() for p in source.rglob('*') if p.is_file()}
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'android'
            prepare.prepare(output)
            self.assertIn('moe.maincore.keine.benchmark', (output / 'config.yaml').read_text())
            changed = {'config.yaml','assets.yaml','scripts/media.shou','scripts/stress.shou','assets/movie.mp4'}
            for relative, content in original.items():
                if relative not in changed:
                    self.assertEqual((output / relative).read_bytes(), content, relative)
                self.assertEqual((source / relative).read_bytes(), content, relative)
            self.assertNotIn('video.', ''.join(p.read_text() for p in (output / 'scripts').glob('*.shou')))
            self.assertIn('bench_stress_android', (output / 'scripts/stress.shou').read_text())
            self.assertFalse((output / 'assets/movie.mp4').exists())
            with self.assertRaises(ValueError): prepare.prepare(output)


if __name__ == '__main__':
    unittest.main()
