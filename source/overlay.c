#include "global.h"

#include "3ds/ipc.h"
#include "3ds/services/gspgpu.h"

#include <memory.h>
#include <stdlib.h>

static unsigned char font[] = {
	0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Char 032 ( )
	0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x00, // Char 033 (!)
	0x6C, 0x6C, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00, // Char 034 (")
	0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00, // Char 035 (#)
	0x18, 0x7E, 0xC0, 0x7C, 0x06, 0xFC, 0x18, 0x00, // Char 036 ($)
	0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00, // Char 037 (%)
	0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00, // Char 038 (&)
	0x30, 0x30, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, // Char 039 (')
	0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00, // Char 040 (()
	0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00, // Char 041 ())
	0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00, // Char 042 (*)
	0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00, // Char 043 (+)
	0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30, // Char 044 (,)
	0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, // Char 045 (-)
	0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, // Char 046 (.)
	0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00, // Char 047 (/)
	0x7C, 0xCE, 0xDE, 0xF6, 0xE6, 0xC6, 0x7C, 0x00, // Char 048 (0)
	0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, // Char 049 (1)
	0x7C, 0xC6, 0x06, 0x7C, 0xC0, 0xC0, 0xFE, 0x00, // Char 050 (2)
	0xFC, 0x06, 0x06, 0x3C, 0x06, 0x06, 0xFC, 0x00, // Char 051 (3)
	0x0C, 0xCC, 0xCC, 0xCC, 0xFE, 0x0C, 0x0C, 0x00, // Char 052 (4)
	0xFE, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00, // Char 053 (5)
	0x7C, 0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00, // Char 054 (6)
	0xFE, 0x06, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x00, // Char 055 (7)
	0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00, // Char 056 (8)
	0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0x06, 0x7C, 0x00, // Char 057 (9)
	0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, // Char 058 (:)
	0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30, // Char 059 (;)
	0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00, // Char 060 (<)
	0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00, // Char 061 (=)
	0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00, // Char 062 (>)
	0x3C, 0x66, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00, // Char 063 (?)
	0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x7E, 0x00, // Char 064 (@)
	0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00, // Char 065 (A)
	0xFC, 0xC6, 0xC6, 0xFC, 0xC6, 0xC6, 0xFC, 0x00, // Char 066 (B)
	0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, // Char 067 (C)
	0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0xCC, 0xF8, 0x00, // Char 068 (D)
	0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xFE, 0x00, // Char 069 (E)
	0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xC0, 0x00, // Char 070 (F)
	0x7C, 0xC6, 0xC0, 0xC0, 0xCE, 0xC6, 0x7C, 0x00, // Char 071 (G)
	0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00, // Char 072 (H)
	0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, // Char 073 (I)
	0x06, 0x06, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, // Char 074 (J)
	0xC6, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00, // Char 075 (K)
	0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00, // Char 076 (L)
	0xC6, 0xEE, 0xFE, 0xFE, 0xD6, 0xC6, 0xC6, 0x00, // Char 077 (M)
	0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00, // Char 078 (N)
	0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, // Char 079 (O)
	0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00, // Char 080 (P)
	0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x06, // Char 081 (Q)
	0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0x00, // Char 082 (R)
	0x7C, 0xC6, 0xC0, 0x7C, 0x06, 0xC6, 0x7C, 0x00, // Char 083 (S)
	0xFF, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, // Char 084 (T)
	0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xFE, 0x00, // Char 085 (U)
	0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x38, 0x00, // Char 086 (V)
	0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0x6C, 0x00, // Char 087 (W)
	0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00, // Char 088 (X)
	0xC6, 0xC6, 0xC6, 0x7C, 0x18, 0x30, 0xE0, 0x00, // Char 089 (Y)
	0xFE, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xFE, 0x00, // Char 090 (Z)
	0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00, // Char 091 ([)
	0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00, // Char 092 (\)
	0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00, // Char 093 (])
	0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, // Char 094 (^)
	0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, // Char 095 (_)
	0x18, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, // Char 096 (`)
	0x00, 0x00, 0x7C, 0x06, 0x7E, 0xC6, 0x7E, 0x00, // Char 097 (a)
	0xC0, 0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0xFC, 0x00, // Char 098 (b)
	0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC6, 0x7C, 0x00, // Char 099 (c)
	0x06, 0x06, 0x06, 0x7E, 0xC6, 0xC6, 0x7E, 0x00, // Char 100 (d)
	0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0x7C, 0x00, // Char 101 (e)
	0x1C, 0x36, 0x30, 0x78, 0x30, 0x30, 0x78, 0x00, // Char 102 (f)
	0x00, 0x00, 0x7E, 0xC6, 0xC6, 0x7E, 0x06, 0xFC, // Char 103 (g)
	0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, // Char 104 (h)
	0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00, // Char 105 (i)
	0x06, 0x00, 0x06, 0x06, 0x06, 0x06, 0xC6, 0x7C, // Char 106 (j)
	0xC0, 0xC0, 0xCC, 0xD8, 0xF8, 0xCC, 0xC6, 0x00, // Char 107 (k)
	0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, // Char 108 (l)
	0x00, 0x00, 0xCC, 0xFE, 0xFE, 0xD6, 0xD6, 0x00, // Char 109 (m)
	0x00, 0x00, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, // Char 110 (n)
	0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, // Char 111 (o)
	0x00, 0x00, 0xFC, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, // Char 112 (p)
	0x00, 0x00, 0x7E, 0xC6, 0xC6, 0x7E, 0x06, 0x06, // Char 113 (q)
	0x00, 0x00, 0xFC, 0xC6, 0xC0, 0xC0, 0xC0, 0x00, // Char 114 (r)
	0x00, 0x00, 0x7E, 0xC0, 0x7C, 0x06, 0xFC, 0x00, // Char 115 (s)
	0x18, 0x18, 0x7E, 0x18, 0x18, 0x18, 0x0E, 0x00, // Char 116 (t)
	0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0x7E, 0x00, // Char 117 (u)
	0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x7C, 0x38, 0x00, // Char 118 (v)
	0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xFE, 0x6C, 0x00, // Char 119 (w)
	0x00, 0x00, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0x00, // Char 120 (x)
	0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0xFC, // Char 121 (y)
	0x00, 0x00, 0xFE, 0x0C, 0x38, 0x60, 0xFE, 0x00, // Char 122 (z)
	0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00, // Char 123 ({)
	0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x00, // Char 124 (|)
	0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00, // Char 125 (})
	0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Char 126 (~)
};

static void ovDrawTranspartBlackRect(u32 addr, u32 stride, u32 format, int r, int c, int h, int w, u8 level)
{
	format &= 0x0f;
	int posC;
	for (posC = c; posC < c + w; posC++) {
		if (format == 3) {
			u16 *sp = (u16 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 2 - 2 * (r + h));
			u16 *spEnd = sp + h;
			while (sp < spEnd)
			{
				u16 pix = *sp;
				u16 r = (pix >> 11) & 0x1f;
				u16 g = (pix >> 6) & 0x1f;
				u16 b = (pix >> 1) & 0x1f;
				pix = ((r >> level) << 11) | ((g >> level) << 6) | ((b >> level) << 1);
				*sp = pix;
				sp++;
			}
		} else if (format == 2) {
			u16 *sp = (u16 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 2 - 2 * (r + h));
			u16 *spEnd = sp + h;
			while (sp < spEnd)
			{
				u16 pix = *sp;
				u16 r = (pix >> 11) & 0x1f;
				u16 g = (pix >> 5) & 0x3f;
				u16 b = (pix & 0x1f);
				pix = ((r >> level) << 11) | ((g >> level) << 5) | (b >> level);
				*sp = pix;
				sp++;
			}
		} else if (format == 1) {
			u8 *sp = (u8 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 3 - 3 * (r + h));
			u8 *spEnd = sp + 3 * h;
			while (sp < spEnd)
			{
				sp[0] >>= level;
				sp[1] >>= level;
				sp[2] >>= level;
				sp += 3;
			}
		} else if (format == 0) {
			u8 *sp = (u8 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 4 - 4 * (r + h));
			u8 *spEnd = sp + 4 * h;
			while (sp < spEnd)
			{
				sp[1] >>= level;
				sp[2] >>= level;
				sp[3] >>= level;
				sp += 4;
			}
		}
	}
}

static void ovDrawPixel(u32 addr, u32 stride, u32 format, int posR, int posC, u8 r, u8 g, u8 b)
{
	format &= 0x0f;
	if (format == 3) {
		u16 pix = ((u16)(r >> 3) << 11) | ((u16)(g >> 3) << 6) | ((u16)(b >> 3) << 1);
		*(u16 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 2 - 2 * posR - 2) = pix;
	} else if (format == 2) {
		u16 pix = ((u16)(r >> 3) << 11) | ((u16)(g >> 2) << 5) | (u16)(b >> 3);
		*(u16 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 2 - 2 * posR - 2) = pix;
	} else if (format == 1) {
		u8 *sp = (u8 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 3 - 3 * posR - 3);
		sp[0] = b;
		sp[1] = g;
		sp[2] = r;
	} else if (format == 0) {
		u8 *sp = (u8 *)(addr + stride * posC + GSP_SCREEN_WIDTH * 4 - 4 * posR - 4);
		sp[1] = b;
		sp[2] = g;
		sp[3] = r;
	}
}

static void ovDrawRect(u32 addr, u32 stride, u32 format, int posR, int posC, int h, int w, u8 r, u8 g, u8 b)
{
	int r_, c_;
	for (c_ = posC; c_ < posC + w; c_++) {
		for (r_ = posR; r_ < posR + h; r_++) {
			ovDrawPixel(addr, stride, format, r_, c_, r, g, b);
		}
	}
}

#define CHAR_WIDTH (8)
#define CHAR_HEIGHT (8)

static void ovDrawChar(u32 addr, u32 stride, u32 format, u8 letter, int y, int x, u8 r, u8 g, u8 b)
{
	int i;
	int k;
	int c;
	unsigned char mask;
	unsigned char l;

	if ((letter < 32) || (letter >= 127)) {
		letter = '.';
	}

	c = (letter - 32) * CHAR_HEIGHT;

	for (i = 0; i < CHAR_HEIGHT; i++) {
		mask = 0b10000000;
		l = font[i + c];
		for (k = 0; k < CHAR_WIDTH; k++) {
			if ((mask >> k) & l) {
				ovDrawPixel(addr, stride, format, i + y, k + x, r, g, b);
			}
		}
	}
}

static void ovDrawString(u32 addr, u32 stride, u32 format, u32 scrnWidth, int posR, int posC, u32 r, u32 g, u32 b, const char *buf)
{
	while (*buf) {
		if ((posR + CHAR_HEIGHT >= (int)GSP_SCREEN_WIDTH) || (posC + CHAR_WIDTH >= (int)scrnWidth))
			return;
		ovDrawChar(addr, stride, format, (u8)*buf, posR, posC, r, g, b);
		buf++;
		posC += CHAR_WIDTH;
	}
}

#define ROW_MARGIN (1)
#define COL_MARGIN ROW_MARGIN
#define ROW_START (ROW_MARGIN)
#define COL_START (COL_MARGIN)

static struct ov_color_t {
	u8 r, g, b;
} text_color_info = {
	255, 255, 255
}, deco_color_err = {
	255, 0, 0
};

static void drawOverlayOnScreenMode0(u32 addr, u32 stride, u32 format, u32 scrnWidth, const char *buf) {
	ovDrawTranspartBlackRect(addr, stride, format, ROW_START, COL_START, CHAR_HEIGHT + ROW_MARGIN * 2, strlen(buf) * CHAR_WIDTH + COL_MARGIN * 2, 1);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf);
}

static void drawOverlayOnScreenMode2(u32 addr, u32 stride, u32 format, u32 scrnWidth, const char *buf, const char *buf2, const char *buf3, const char *buf4, const char *buf5/*, const char *buf6*/) {
	int len = strlen(buf);
	int len2 = strlen(buf2);
	int len3 = strlen(buf3);
	int len4 = strlen(buf4);
	int len5 = strlen(buf5);
	// int len6 = strlen(buf6);
	ovDrawTranspartBlackRect(addr, stride, format, ROW_START, COL_START, CHAR_HEIGHT * 5 + ROW_MARGIN * 6, MAX(MAX(MAX(MAX(len, len2), len3), len4), len5) * CHAR_WIDTH + COL_MARGIN * 2, 1);
	// ovDrawTranspartBlackRect(addr, stride, format, ROW_START, COL_START, CHAR_HEIGHT * 6 + ROW_MARGIN * 7, MAX(MAX(MAX(MAX(MAX(len, len2), len3), len4), len5), len6) * CHAR_WIDTH + COL_MARGIN * 2, 1);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN * 2 + CHAR_HEIGHT, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf2);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN * 3 + CHAR_HEIGHT * 2, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf3);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN * 4 + CHAR_HEIGHT * 3, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf4);
	ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN * 5 + CHAR_HEIGHT * 4, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf5);
	// ovDrawString(addr, stride, format, scrnWidth, ROW_START + ROW_MARGIN * 6 + CHAR_HEIGHT * 5, COL_START + COL_MARGIN, text_color_info.r, text_color_info.g, text_color_info.b, buf6);
}

static void drawOverlayOnScreenDefault(u32 addr, u32 stride, u32 format) {
	ovDrawRect(addr, stride, format, ROW_START, COL_START, ROW_MARGIN * 2, COL_MARGIN * 2, deco_color_err.r, deco_color_err.g, deco_color_err.b);
}

static int plgDrawOverlayStats(u32 isDisplay1, u32 addr, u32 addrB, u32 stride, u32 format) {
	if ((addr >= 0x1f000000) && (addr < 0x1f600000)) {
		if (!plgHasVRAMAccess) {
			return -1;
		}
	}

	int s = isDisplay1 ? 1 : 0;
	int scrnWidth = s == 0 ? GSP_SCREEN_HEIGHT_TOP : GSP_SCREEN_HEIGHT_BOTTOM;
	svcInvalidateProcessDataCache(CUR_PROCESS_HANDLE, (u32)addr, stride * scrnWidth);
	if ((isDisplay1 == 0) && (addrB) && (addrB != addr)) {
		svcInvalidateProcessDataCache(CUR_PROCESS_HANDLE, (u32)addrB, stride * scrnWidth);
	}

	format &= 0x0f;

	OVERLAY_STATS_INFO *ov = &nsConfig->ovStats;
	struct overlay_stats_screen_t *stats = &ov->s[s];
#define PARTS(n) (u32)((n) / 1000), (u32)(abs(n) % 1000)
	switch (ov->kcp_mode) {
		case 0: {
			char buf[LOCAL_OPT_TEXT_BUF_SIZE];
			xsnprintf(buf, LOCAL_OPT_TEXT_BUF_SIZE,
				"%4"PRIu32".%03"PRId32" %8"PRIu32" %"PRIu32,
				PARTS(stats->comp_size),
				(u32)((u64)stats->frame_time * 1000000 / SYSCLOCK_ARM11),
				format);
			drawOverlayOnScreenMode0(addr, stride, format, scrnWidth, buf);
			if (isDisplay1 == 0 && addrB && addrB != addr)  {
				drawOverlayOnScreenMode0(addrB, stride, format, scrnWidth, buf);
			}
		}
			break;
		case 1: {
			char buf[LOCAL_OPT_TEXT_BUF_SIZE];
			xsnprintf(buf, LOCAL_OPT_TEXT_BUF_SIZE,
				"%4"PRIu32".%03"PRId32" %8"PRIu32" %"PRIu32" %"PRIu32".%03"PRIu32" MB/s",
				PARTS(stats->comp_size),
				(u32)((u64)stats->frame_time * 1000000 / SYSCLOCK_ARM11),
				format,
				ov->kcp_qos / 1024 / 1024, ov->kcp_qos / 1024 % 1024 * 1000 / 1024);
			drawOverlayOnScreenMode0(addr, stride, format, scrnWidth, buf);
			if (isDisplay1 == 0 && addrB && addrB != addr)  {
				drawOverlayOnScreenMode0(addrB, stride, format, scrnWidth, buf);
			}
		}
			break;
#define PRINT_DELTA_Q(buf, f) \
	xsnprintf(buf, LOCAL_OPT_TEXT_BUF_SIZE, \
	"%3"PRId32".%03"PRId32" %4"PRId32".%03"PRId32" %4"PRId32".%03"PRId32, \
	PARTS((f).m), PARTS((f).p), PARTS((f).d))
		case 2: {
			char buf[LOCAL_OPT_TEXT_BUF_SIZE];
			xsnprintf(buf, LOCAL_OPT_TEXT_BUF_SIZE,
				"%4"PRIu32".%03"PRId32" %8"PRIu32" %"PRIu32" %"PRIu32".%03"PRIu32" MB/s",
				PARTS(stats->comp_size),
				(u32)((u64)stats->frame_time * 1000000 / SYSCLOCK_ARM11),
				format,
				ov->kcp_qos / 1024 / 1024, ov->kcp_qos / 1024 % 1024 * 1000 / 1024);
			char buf2[LOCAL_OPT_TEXT_BUF_SIZE];
			xsnprintf(buf2, LOCAL_OPT_TEXT_BUF_SIZE,
				"%4"PRId32".%03"PRId32" %4"PRId32".%03"PRId32" %4"PRId32".%03"PRId32" %2"PRId32,
				PARTS(stats->delta_q.qb), PARTS(stats->delta_q.qc), PARTS(stats->delta_q.nbits), stats->delta_q.qd);
			char buf3[LOCAL_OPT_TEXT_BUF_SIZE];
			PRINT_DELTA_Q(buf3, stats->delta_q.f[0]);
			char buf4[LOCAL_OPT_TEXT_BUF_SIZE];
			PRINT_DELTA_Q(buf4, stats->delta_q.f[1]);
			char buf5[LOCAL_OPT_TEXT_BUF_SIZE];
			PRINT_DELTA_Q(buf5, stats->delta_q.f[2]);
			// char buf6[LOCAL_OPT_TEXT_BUF_SIZE];
			// PRINT_DELTA_Q(buf4, stats->delta_q.f[3]);
			drawOverlayOnScreenMode2(addr, stride, format, scrnWidth, buf, buf2, buf3, buf4, buf5/*, buf6*/);
			if (isDisplay1 == 0 && addrB && addrB != addr)  {
				drawOverlayOnScreenMode2(addrB, stride, format, scrnWidth, buf, buf2, buf3, buf4, buf5/*, buf6*/);
			}
		}
#undef PRINT_DELTA_Q
			break;
		default:
			drawOverlayOnScreenDefault(addr, stride, format);
			if (isDisplay1 == 0 && addrB && addrB != addr)  {
				drawOverlayOnScreenDefault(addrB, stride, format);
			}
			break;
	}
#undef PARTS

	return format < 4;
}

int plgOverlayStatus;
int plgHasVRAMAccess;
int plgHasOverlay;

static u32 *plgOverlayThreadStack;
static Handle *plgOverlayEvent;
static u32 rpPortIsTop;

typedef u32 (*SetBufferSwapTypedef)(u32 isDisplay1, u32 a2, u32 addr, u32 addrB, u32 width, u32 a6, u32 a7);
typedef u32 (*SetBufferSwapTypedef2)(u32 r0, u32 *params, u32 isBottom, u32 arg);
static RT_HOOK SetBufferSwapHook;

static int rpPortSend(u32 isTop);
static void plgSetBufferSwapCommon(u32 isDisplay1, u32 addr, u32 addrB, u32 stride, u32 format) {
	if (plgLoaderEx->remotePlayBoost) {
		if (plgOverlayEvent && *plgOverlayEvent) {
			ASR(&rpPortIsTop, isDisplay1 ? 0 : 1);
			s32 ret;
			ret = svcSignalEvent(*plgOverlayEvent);
			if (ret != 0) {
				nsDbgPrint("plgOverlayEvent signal failed: %08"PRIx32"\n", ret);
			}
		} else {
			rpPortSend(isDisplay1 ? 0 : 1);
		}
	} else {
		u32 pid = getCurrentProcessId();
		rpSetGamePid(pid == ntrConfig->HomeMenuPid ? 0 : pid, 1);
	}

	int isDirty = 0;
	if (plgLoaderEx->overlayStats) {
		isDirty = plgDrawOverlayStats(isDisplay1, addr, addrB, stride, format) == 0;
	}

	plgSetBufferSwapHandle(isDisplay1, addr, addrB, stride, format, isDirty);
}

static u32 plgSetBufferSwapCallback(u32 isDisplay1, u32 a2, u32 addr, u32 addrB, u32 stride, u32 format, u32 a7) {
	if (addr)
		plgSetBufferSwapCommon(isDisplay1, addr, addrB, stride, format);
	u32 ret = ((SetBufferSwapTypedef)SetBufferSwapHook.callCode)(isDisplay1, a2, addr, addrB, stride, format, a7);
	return ret;
}

// taken from CTRPF
static u32 plgSetBufferSwapCallback2(u32 r0, u32 *params, u32 isDisplay1, u32 arg) {
	if (params)
	{
		// u32 isBottom = params[0];
		u32 addr = params[1];
		// void *addrB = params[2]; possible, not confirmed
		u32 stride = params[3];
		u32 format = params[4] & 0xF;

		if (addr)
			plgSetBufferSwapCommon(isDisplay1, addr, 0, stride, format);
	}

	u32 ret = ((SetBufferSwapTypedef2)SetBufferSwapHook.callCode)(r0, params, isDisplay1, arg);
	return ret;
}

static int rpPortSend(u32 isTop) {
	Handle hClient = rpGetPortHandle();
	if (!hClient)
		return -1;

	u32* cmdbuf = getThreadCommandBuffer();
	cmdbuf[0] = IPC_MakeHeader(SVC_NWM_CMD_OVERLAY_CALLBACK, 1, 2);
	cmdbuf[1] = isTop;
	cmdbuf[2] = IPC_Desc_CurProcessId();

	s32 ret = svcSendSyncRequest(hClient);
	if (ret != 0) {
		nsDbgPrint("Send port request failed: %08"PRIx32"\n", ret);
		return -1;
	}
	return 0;
}

void __system_initSyscalls(void);
static void plgOverlayThread(void *fp) {
	__system_initSyscalls();

	if (!fp) {
		while (1) {
			rpPortSend(-1);
			svcSleepThread(1000000000);
		}
	}

	int ret;
	while (1) {
		ret = svcWaitSynchronization(*plgOverlayEvent, 1000000000);
		if (ret != 0) {
			if (ret == RES_TIMEOUT) {
				rpPortSend(-1);
				continue;
			}
			svcSleepThread(1000000000);
			continue;
		}
		if (rpPortSend(ALR(&rpPortIsTop)) != 0) {
			svcSleepThread(1000000000);
		}
	}

	svcExitThread();
}

static u32 plgSearchReverse(u32 endAddr, u32 startAddr, u32 pat) {
	if (endAddr == 0) {
		return 0;
	}
	while (endAddr >= startAddr) {
		if (*(u32 *)endAddr == pat) {
			return endAddr;
		}
		endAddr -= 4;
	}
	return 0;
}

static u32 plgSearchBytes(u32 startAddr, u32 endAddr, const u32 *pat, int patlen) {
	u32 lastPage = 0;
	u32 pat0 = pat[0];

	while (1) {
		if (endAddr) {
			if (startAddr >= endAddr) {
				return 0;
			}
		}
		u32 currentPage = rtGetPageOfAddress(startAddr);
		if (currentPage != lastPage) {
			lastPage = currentPage;
			if (rtCheckMemory(currentPage, 0x1000, MEMPERM_READ) != 0) {
				return 0;
			}
		}
		if (*(u32 *)startAddr == pat0) {
			if (memcmp((void *)startAddr, pat, patlen) == 0) {
				return startAddr;
			}
		}
		startAddr += 4;
	}
	return 0;
}

static int plgCreateOverlayThread(u32 fp, u32 *stack) {
	if (!plgOverlayThreadStack) {
		if (stack) {
			plgOverlayThreadStack = stack;
		} else {
			plgOverlayThreadStack = (void *)plgRequestMemoryFromPool(SMALL_STACK_SIZE, 1);
			if (!plgOverlayThreadStack) {
				return -1;
			}
		}
	}
	plgOverlayEvent = plgOverlayThreadStack;
	s32 ret;
	ret = svcCreateEvent(plgOverlayEvent, RESET_ONESHOT);
	if (ret != 0) {
		nsDbgPrint("Create plgOverlayEvent failed: %08"PRIx32"\n", ret);
		*plgOverlayEvent = 0;
		return ret;
	}
	Handle hThread;
	ret = svcCreateThread(&hThread, plgOverlayThread, fp, &plgOverlayThreadStack[(SMALL_STACK_SIZE / 4) - 10], 0x18, -2);
	if (ret != 0) {
		nsDbgPrint("Create plgOverlayThread failed: %08"PRIx32"\n", ret);
		return ret;
	}
	return 0;
}

void plgInitScreenOverlay(u32 *stack) {
	if (plgLoaderEx->CTRPFCompat) {
		plgOverlayStatus = 2;
		return;
	}

	if (plgOverlayStatus) {
		return;
	}
	plgOverlayStatus = 2;

	if (rtCheckMemory(0x1F000000, 0x00600000, MEMPERM_READWRITE) == 0)
		plgHasVRAMAccess = 1;

	static const u32 pat[] = { 0xe1833000, 0xe2044cff, 0xe3c33cff, 0xe1833004, 0xe1824f93 };
	static const u32 pat2[] = { 0xe8830e60, 0xee078f9a, 0xe3a03001, 0xe7902104 };
	static const u32 pat3[] = { 0xee076f9a, 0xe3a02001, 0xe7901104, 0xe1911f9f, 0xe3c110ff };

	u32 addr, fp, fp2;
	addr = plgSearchBytes(0x00100000, 0, pat, sizeof(pat));
	if (!addr) {
		addr = plgSearchBytes(0x00100000, 0, pat2, sizeof(pat2));
	}
	fp = plgSearchReverse(addr, addr - 0x400, 0xe92d5ff0);
	if (!fp) {
		addr = plgSearchBytes(0x00100000, 0, pat3, sizeof(pat3));
		fp = plgSearchReverse(addr, addr - 0x400, 0xe92d47f0);
	}

	// taken from CTRPF
	static const u32 pat4[] = { 0xE3A00000, 0xEE070F9A, 0xE3A00001, 0xE7951104 };

	if (fp) {
		fp2 = 0;
	} else {
		addr = plgSearchBytes(0x00100000, 0, pat4, sizeof(pat4));
		fp2 = plgSearchReverse(addr, addr - 0x400, 0xE92D4070);
	}

	nsDbgPrint("Overlay addr: %"PRIx32"; fp: %"PRIx32"; fp2: %"PRIx32"\n", addr, fp, fp2);

	if (plgLoaderEx->remotePlayBoost && plgCreateOverlayThread(fp || fp2, stack) != 0) {
		nsDbgPrint("Overlay thread create failed\n");
		// return;
	}

	if (fp) {
		rtInitHook(&SetBufferSwapHook, fp, (u32)plgSetBufferSwapCallback);
		rtEnableHook(&SetBufferSwapHook);
		plgOverlayStatus = 1;
	} else if (fp2) {
		rtInitHook(&SetBufferSwapHook, fp2, (u32)plgSetBufferSwapCallback2);
		rtEnableHook(&SetBufferSwapHook);
		plgOverlayStatus = 1;
	}
}

void plgInitScreenOverlayDirectly(u32 funcAddr) {
	if (rtCheckMemory(0x1F000000, 0x00600000, MEMPERM_READWRITE) == 0)
		plgHasVRAMAccess = 1;

	plgCreateOverlayThread(1, NULL);
	rtInitHook(&SetBufferSwapHook, funcAddr, (u32)plgSetBufferSwapCallback);
	rtEnableHook(&SetBufferSwapHook);
}
