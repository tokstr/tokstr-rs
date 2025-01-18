CFLAGS = $(pkg-config --cflags libavformat libavcodec libavutil libswscale)
LDFLAGS = $(pkg-config --libs libavformat libavcodec libavutil libswscale)

libextractframe.so: extract_jpeg_frame.o
    $(CC) -shared -o $@ extract_jpeg_frame.o $(LDFLAGS)

extract_jpeg_frame.o: extract_jpeg_frame.c
    $(CC) -c $(CFLAGS) extract_jpeg_frame.c -o extract_jpeg_frame.o

clean:
    rm -f *.o libextractframe.so
