#!/usr/bin/env python3
"""Generate source mutants for Python files in the same JSON shape as
`cargo mutants --list --json` (file, function, genre, replacement, span).

Usage: python_mutants.py apps/server/server.py [...] > python-mutants.json

Operators: comparison negation, and/or swap, `not` removal, True/False
swap, call-statement deletion, and early `return None` for each function.
Spans use 1-based lines and columns with an exclusive end, like cargo-mutants.
"""
import ast
import json
import sys

NEGATE = {ast.Eq: '!=', ast.NotEq: '==', ast.Lt: '>=', ast.GtE: '<', ast.Gt: '<=',
          ast.LtE: '>', ast.In: 'not in', ast.NotIn: 'in', ast.Is: 'is not', ast.IsNot: 'is'}


def span(sl, sc, el, ec):
    return {'start': {'line': sl, 'column': sc + 1}, 'end': {'line': el, 'column': ec + 1}}


def generate(path):
    source = open(path).read()
    lines = source.splitlines()
    tree = ast.parse(source)
    parents = {}
    for node in ast.walk(tree):
        for child in ast.iter_child_nodes(node):
            parents[child] = node

    def function_of(node):
        while node in parents:
            node = parents[node]
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                return node.name
        return '<module>'

    out = []

    def add(node, genre, sp, replacement, text):
        line = sp['start']['line']
        out.append({'package': 'server', 'file': path, 'function': {'function_name': function_of(node)},
                    'genre': genre, 'replacement': replacement, 'span': sp,
                    'name': f'{path}:{line}: {text}'})

    for node in ast.walk(tree):
        if isinstance(node, ast.Compare) and len(node.ops) == 1 and type(node.ops[0]) in NEGATE:
            left, right = node.left, node.comparators[0]
            if left.end_lineno != right.lineno:
                continue
            between = lines[left.end_lineno - 1][left.end_col_offset:right.col_offset]
            op = between.strip()
            if not op:
                continue
            start = left.end_col_offset + between.index(op)
            new = NEGATE[type(node.ops[0])]
            add(node, 'CompareNegation', span(left.end_lineno, start, left.end_lineno, start + len(op)),
                new, f'replace {op} with {new}')
        elif isinstance(node, ast.BoolOp) and len(node.values) == 2:
            a, b = node.values
            if a.end_lineno != b.lineno:
                continue
            between = lines[a.end_lineno - 1][a.end_col_offset:b.col_offset]
            op = 'and' if isinstance(node.op, ast.And) else 'or'
            if op not in between:
                continue
            start = a.end_col_offset + between.index(op)
            new = 'or' if op == 'and' else 'and'
            add(node, 'BoolOperator', span(a.end_lineno, start, a.end_lineno, start + len(op)),
                new, f'replace {op} with {new}')
        elif isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            operand = node.operand
            if operand.lineno != node.lineno:
                continue
            add(node, 'UnaryNot', span(node.lineno, node.col_offset, operand.lineno, operand.col_offset),
                '', 'delete not')
        elif isinstance(node, ast.Constant) and isinstance(node.value, bool):
            new = str(not node.value)
            add(node, 'BoolConstant', span(node.lineno, node.col_offset, node.end_lineno, node.end_col_offset),
                new, f'replace {node.value} with {new}')
        elif (isinstance(node, ast.Expr) and isinstance(node.value, ast.Call)
              and node.lineno == node.end_lineno):
            add(node, 'DeleteCall', span(node.lineno, node.col_offset, node.end_lineno, node.end_col_offset),
                'pass', 'delete call statement')
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            body = node.body
            first = body[0]
            if (isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant)
                    and isinstance(first.value.value, str)):
                if len(body) == 1:
                    continue
                first = body[1]
            indent = ' ' * first.col_offset
            out.append({'package': 'server', 'file': path, 'function': {'function_name': node.name},
                        'genre': 'FnValue', 'replacement': 'return None',
                        'span': span(first.lineno, first.col_offset, first.lineno, first.col_offset),
                        'insert': 'return None\n' + indent,
                        'name': f'{path}:{first.lineno}: replace {node.name} body with return None'})
    out.sort(key=lambda m: (m['span']['start']['line'], m['span']['start']['column']))
    return out


if __name__ == '__main__':
    mutants = []
    for path in sys.argv[1:]:
        mutants += generate(path)
    json.dump(mutants, sys.stdout, indent=1)
