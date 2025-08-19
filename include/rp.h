#ifndef RP_H
#define RP_H

#include "3ds/types.h"
#include "constants.h"

typedef struct {
	u32 chromaSs;
	u32 downSample;
	u32 fpsLimit;
	u32 noSkipFrame;
} RP_SCREEN_CONFIG;

typedef struct {
	u32 mode; // screen priority
	u32 quality;
	u32 qos; // in bytes per second
	u32 coreCount;
	u32 dstPort;
	u32 dstAddr;
	u32 threadPriority;
	u32 gamePid;
	u32 separateScreenConfig;
	RP_SCREEN_CONFIG screens[RP_SCREEN_COUNT];
} RP_CONFIG;

int rpStartupFromMenu(RP_CONFIG *config);
void rpStartup(u8 *buf);
void rpCheckReliableStreamForNFC(void);

typedef u32 (*sendPacketTypedef)(u8 *, u32);
extern sendPacketTypedef nwmSendPacket;

#endif
