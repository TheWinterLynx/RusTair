from pathlib import Path

path = Path('.github/scripts/finish_memory_ready.py')
code = path.read_text(encoding='utf-8')
old = '''    "            let ready = self.machine.bus.cpu_control_lines().ready;",\n    "            let ready = self.machine.bus.cycle_front_panel_ready_input();",\n    2,\n)'''
new = '''    "            let ready = self.machine.bus.cpu_control_lines().ready;",\n    "            let ready = self.machine.bus.cycle_front_panel_ready_input();",\n    3,\n)'''
if old not in code:
    raise SystemExit('expected READY replacement count anchor not found in finish_memory_ready.py')
code = code.replace(old, new, 1)
exec(compile(code, str(path), 'exec'))
