# Prints what a webpad virtual pad emits on Linux: finds the device with phys webpad/P<n>
# and decodes its evdev events. Needs read access to the event node (docs/platforms.md).
#   python3 -u tools/read-pad.py [seconds] [player]
import os, re, select, struct, sys, time
deadline = time.time() + float(sys.argv[1] if len(sys.argv) > 1 else 8)
player = sys.argv[2] if len(sys.argv) > 2 else '1'
node = None
while node is None and time.time() < deadline:
    for block in open('/proc/bus/input/devices').read().split('\n\n'):
        if f'P: Phys=webpad/P{player}\n' in block + '\n':
            print(block.strip().replace('\n', ' | '))
            node = '/dev/input/' + re.search(r'event\d+', block).group(0)
    time.sleep(0.1)
if not node: sys.exit(f'no webpad/P{player} device')
fd = None
while fd is None and time.time() < deadline:  # wait for the udev rule to apply the ACL
    try: fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK)
    except PermissionError: time.sleep(0.05)
if fd is None: sys.exit('no read access to ' + node)
print('opened', node)
names = {(1,0x130):'BTN_SOUTH',(1,0x131):'BTN_EAST',(1,0x133):'BTN_NORTH',(1,0x134):'BTN_WEST',(1,0x13b):'BTN_START',
         (3,0):'ABS_X',(3,1):'ABS_Y',(3,2):'ABS_Z',(3,3):'ABS_RX',(3,4):'ABS_RY',(3,5):'ABS_RZ',(3,0x10):'ABS_HAT0X',(3,0x11):'ABS_HAT0Y'}
while time.time() < deadline:
    if not select.select([fd], [], [], 0.2)[0]: continue
    try: data = os.read(fd, 24 * 64)
    except BlockingIOError: continue
    except OSError as e: print('device gone:', e); break
    for i in range(0, len(data), 24):
        _, _, t, c, v = struct.unpack('qqHHi', data[i:i+24])
        if t: print(names.get((t, c), f'type{t}/code{c:#x}'), v)
