#!/usr/bin/env python3
"""Run one tests/<suite>.py module and stream per-test outcomes as JSON lines.

Usage: unittest_json.py <suite.py> <output.jsonl>

Each line is {"test": "<Class>.<method>", "status": "ok|fail|error|skip"}.
Lines are flushed as tests finish, so a suite killed by a timeout still
reports the tests that completed before it hung.
"""
import importlib.util
import json
import sys
import unittest
from pathlib import Path


def main():
    script = Path(sys.argv[1]).resolve()
    out = open(sys.argv[2], 'w', buffering=1)
    sys.path.insert(0, str(script.parent))
    spec = importlib.util.spec_from_file_location(script.stem, script)
    module = importlib.util.module_from_spec(spec)
    sys.modules[script.stem] = module
    spec.loader.exec_module(module)

    def name(test):
        test_id = test.id()
        # Drop the module prefix; keep Class.method (or the setUpClass marker).
        short = test_id.split('.', 1)[1] if '.' in test_id else test_id
        method = getattr(test, '_testMethodName', None)
        if method:
            # Tests inherited from (or imported with) another suite's class are
            # tagged with the defining class so reports can collapse duplicates.
            owner = next((c for c in type(test).__mro__ if method in c.__dict__), None)
            if owner is not None and owner.__module__ != script.stem:
                short += f' [from {owner.__module__}.{owner.__qualname__}]'
        return short

    class Result(unittest.TextTestResult):
        def _emit(self, test, status):
            out.write(json.dumps({'test': name(test), 'status': status}) + '\n')

        def addSuccess(self, test):
            super().addSuccess(test)
            self._emit(test, 'ok')

        def addFailure(self, test, err):
            super().addFailure(test, err)
            self._emit(test, 'fail')

        def addError(self, test, err):
            super().addError(test, err)
            self._emit(test, 'error')

        def addSkip(self, test, reason):
            super().addSkip(test, reason)
            self._emit(test, 'skip')

        def addExpectedFailure(self, test, err):
            super().addExpectedFailure(test, err)
            self._emit(test, 'ok')

        def addUnexpectedSuccess(self, test):
            super().addUnexpectedSuccess(test)
            self._emit(test, 'fail')

        def addSubTest(self, test, subtest, err):
            super().addSubTest(test, subtest, err)
            if err is not None:
                self._emit(test, 'fail')

    suite = unittest.defaultTestLoader.loadTestsFromModule(module)
    result = unittest.TextTestRunner(resultclass=Result, verbosity=1).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == '__main__':
    sys.exit(main())
