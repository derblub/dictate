#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Prüft den OpenGL-Treiber des Rechners ohne Fenster: EGL-Kontext auf dem Gerät (EGL_EXT_platform_device), ein
Shader-Programm übersetzen, ein Dreieck in einen Puffer zeichnen und einen Bildpunkt zurücklesen.

    tests/systemd/under-unit.py dictate-ui --rw DIR -- /usr/bin/env XDG_CACHE_HOME=DIR tests/systemd/gl-check.py

Zweck (docs/systemd-hardening.md): Die Wayland-Tests laufen mit Software-Rendering (pixman) und zeigen darum nicht, ob
der Grafiktreiber unter dem Systemaufruf-Filter, im User-Namespace und mit nur lesbarem Heimatordner arbeitet. Dieses
Skript lädt denselben Treiber wie GTK in `dictate-ui` und lässt ihn Shader übersetzen. Es öffnet kein Fenster und
spricht mit keinem Compositor. Ausgabe: Renderer, Bildpunkt, Zahl der schreib- und ausführbaren Abbildungen des
Prozesses und die Dateien, die der Treiber unter `XDG_CACHE_HOME` angelegt hat. Endet mit 1, wenn etwas scheitert.
"""

import ctypes
import os
import sys

EGL_PLATFORM_DEVICE_EXT = 0x313F
EGL_OPENGL_ES_API = 0x30A0
EGL_NONE = 0x3038
EGL_SURFACE_TYPE, EGL_PBUFFER_BIT = 0x3033, 0x0001
EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT = 0x3040, 0x0004
EGL_WIDTH, EGL_HEIGHT = 0x3057, 0x3056
EGL_CONTEXT_CLIENT_VERSION = 0x3098
GL_RENDERER, GL_VERTEX_SHADER, GL_FRAGMENT_SHADER = 0x1F01, 0x8B31, 0x8B30
GL_COMPILE_STATUS, GL_LINK_STATUS = 0x8B81, 0x8B82
GL_FLOAT, GL_TRIANGLES, GL_RGBA, GL_UNSIGNED_BYTE, GL_COLOR_BUFFER_BIT = 0x1406, 0x0004, 0x1908, 0x1401, 0x4000

VERT = b"attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }"
FRAG = b"precision mediump float; uniform vec4 c; void main() { gl_FragColor = c; }"


def fail(what):
    sys.exit(f"gl-check: {what}")


def ints(*values):
    return (ctypes.c_int * len(values))(*values)


def main():
    egl = ctypes.CDLL("libEGL.so.1")
    gl = ctypes.CDLL("libGLESv2.so.2")
    for fn in ("eglGetProcAddress", "eglCreatePbufferSurface", "eglCreateContext"):
        getattr(egl, fn).restype = ctypes.c_void_p
    egl.eglGetProcAddress.argtypes = [ctypes.c_char_p]

    def extension(name, restype, *argtypes):
        addr = egl.eglGetProcAddress(name)
        if not addr:
            fail(f"EGL has no {name.decode()}")
        return ctypes.CFUNCTYPE(restype, *argtypes)(addr)

    query_devices = extension(b"eglQueryDevicesEXT", ctypes.c_uint, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)
    platform_display = extension(b"eglGetPlatformDisplayEXT", ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p)
    egl.eglInitialize.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    egl.eglChooseConfig.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
    egl.eglCreatePbufferSurface.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    egl.eglCreateContext.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
    egl.eglMakeCurrent.argtypes = [ctypes.c_void_p] * 4
    egl.eglTerminate.argtypes = [ctypes.c_void_p]
    gl.glGetString.restype = ctypes.c_char_p

    devices = (ctypes.c_void_p * 8)()
    count = ctypes.c_int(0)
    if not query_devices(8, devices, ctypes.byref(count)) or count.value == 0:
        fail("no EGL device")
    display = platform_display(EGL_PLATFORM_DEVICE_EXT, devices[0], None)
    if not display or not egl.eglInitialize(display, None, None):
        fail("eglInitialize failed")
    egl.eglBindAPI(EGL_OPENGL_ES_API)
    config = ctypes.c_void_p()
    if not egl.eglChooseConfig(display, ints(EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE), ctypes.byref(config), 1, ctypes.byref(count)) or count.value == 0:
        fail("no EGL config")
    surface = egl.eglCreatePbufferSurface(display, config, ints(EGL_WIDTH, 16, EGL_HEIGHT, 16, EGL_NONE))
    context = egl.eglCreateContext(display, config, None, ints(EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE))
    if not surface or not context or not egl.eglMakeCurrent(display, surface, surface, context):
        fail("no context")
    renderer = (gl.glGetString(GL_RENDERER) or b"?").decode()

    def shader(kind, source):
        s = gl.glCreateShader(kind)
        gl.glShaderSource(s, 1, ctypes.byref(ctypes.c_char_p(source)), None)
        gl.glCompileShader(s)
        ok = ctypes.c_int(0)
        gl.glGetShaderiv(s, GL_COMPILE_STATUS, ctypes.byref(ok))
        if not ok.value:
            fail("shader does not compile")
        return s

    program = gl.glCreateProgram()
    gl.glAttachShader(program, shader(GL_VERTEX_SHADER, VERT))
    gl.glAttachShader(program, shader(GL_FRAGMENT_SHADER, FRAG))
    gl.glBindAttribLocation(program, 0, b"p")
    gl.glLinkProgram(program)
    ok = ctypes.c_int(0)
    gl.glGetProgramiv(program, GL_LINK_STATUS, ctypes.byref(ok))
    if not ok.value:
        fail("program does not link")
    gl.glUseProgram(program)
    gl.glUniform4f.argtypes = [ctypes.c_int] + [ctypes.c_float] * 4
    gl.glUniform4f(gl.glGetUniformLocation(program, b"c"), 0.0, 1.0, 0.0, 1.0)
    gl.glClearColor.argtypes = [ctypes.c_float] * 4
    gl.glClearColor(1.0, 0.0, 0.0, 1.0)
    gl.glClear(GL_COLOR_BUFFER_BIT)
    triangle = (ctypes.c_float * 6)(-1.0, -1.0, 3.0, -1.0, -1.0, 3.0)
    gl.glVertexAttribPointer(0, 2, GL_FLOAT, 0, 0, triangle)
    gl.glEnableVertexAttribArray(0)
    gl.glDrawArrays(GL_TRIANGLES, 0, 3)
    pixel = (ctypes.c_ubyte * 4)()
    gl.glReadPixels(8, 8, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel)
    gl.glFinish()
    rwx = sum(1 for line in open("/proc/self/maps") if line.split()[1].startswith("rwx"))
    egl.eglMakeCurrent(display, None, None, None)
    egl.eglTerminate(display)
    cache = os.environ.get("XDG_CACHE_HOME", "")
    cached = sum(len(files) for _, _, files in os.walk(cache)) if cache else 0
    print(f"renderer: {renderer}")
    print(f"pixel: {tuple(pixel)} (green expected)")
    print(f"writable and executable mappings: {rwx}")
    print(f"files below XDG_CACHE_HOME: {cached}")
    if tuple(pixel) != (0, 255, 0, 255):
        fail("the triangle was not drawn")


if __name__ == "__main__":
    main()
