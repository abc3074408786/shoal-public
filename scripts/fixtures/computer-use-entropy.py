"""Synthetic multi-monitor capture through the real service/relay; never input."""
import random
from PIL import Image
from velella_computer_use import server, windows

image = Image.frombytes('RGB', (3840, 2160), random.Random(1729).randbytes(3840 * 2160 * 3))
geometry = windows.DesktopGeometry(-1920, -120, 3840, 2160)
windows.capture_desktop = lambda: (image, geometry)
raise SystemExit(server.main())
