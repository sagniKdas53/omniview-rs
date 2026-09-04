#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include "c_xdnd.h"
#include "../xdnd.h"

static char *g_xdnd_data = NULL;

static void widget_get_data(DndClass *dnd, Window window, unsigned char **data, int *length, Atom type) {
    if (g_xdnd_data) {
        *data = (unsigned char *)g_xdnd_data;
        *length = (int)strlen(g_xdnd_data);
    } else {
        *data = NULL;
        *length = 0;
    }
}

int xdnd_start_drag_uris(const char **uris, int count) {
    if (!uris || count <= 0) return 0;

    Display *display = XOpenDisplay(NULL);
    if (!display) return 0;

    size_t total = 1;
    for (int i = 0; i < count; i++) {
        total += strlen(uris[i]) + 4;
    }
    char *buffer = (char *)malloc(total);
    buffer[0] = '\0';
    for (int i = 0; i < count; i++) {
        strcat(buffer, uris[i]);
        strcat(buffer, "\r\n");
    }

    if (g_xdnd_data) free(g_xdnd_data);
    g_xdnd_data = buffer;

    DndClass dnd;
    memset(&dnd, 0, sizeof(dnd));
    xdnd_init(&dnd, display);
    dnd.widget_get_data = widget_get_data;

    Atom typelist[2];
    typelist[0] = XInternAtom(display, "text/uri-list", False);
    typelist[1] = 0;

    Window root = DefaultRootWindow(display);
    Atom result = xdnd_drag(&dnd, root, dnd.XdndActionCopy, typelist);

    if (g_xdnd_data) {
        free(g_xdnd_data);
        g_xdnd_data = NULL;
    }

    XCloseDisplay(display);
    return (result != 0);
}
