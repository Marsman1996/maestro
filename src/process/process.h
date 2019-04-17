#ifndef PROCESS_H
# define PROCESS_H

# include <stdint.h>

# define PROCESS_MAX_PID	32768

typedef int16_t pid_t;

typedef struct
{
	pid_t pid;
	// TODO data

	uint32_t *paging_dir;
} process_t;

void process_init();
pid_t kfork();
process_t *get_process(const pid_t pid);

#endif
