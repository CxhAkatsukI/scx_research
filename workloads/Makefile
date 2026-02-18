CC = gcc
CFLAGS = -O2 -pthread -Wall

all: hog critical critical_2

hog: hog.c
	$(CC) $(CFLAGS) -o hog hog.c

critical: critical.c
	$(CC) $(CFLAGS) -o critical critical.c

critical_2: critical_2.c
	$(CC) $(CFLAGS) -o critical_2 critical_2.c

clean:
	rm -f hog critical critical_2