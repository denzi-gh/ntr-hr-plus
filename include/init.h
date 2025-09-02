#ifndef INIT_H
#define INIT_H

#include "ntr_config.h"
int setUpReturn(void);
void startupInit(void);
void loadParams(NTR_CONFIG *ntrCfg);
void initSharedFunc(void);
int plgLoaderInfoAlloc(void);
extern int ntr_loader_region;

#endif
