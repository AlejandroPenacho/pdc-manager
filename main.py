import socket

streams = [
    socket.socket(family=socket.AF_UNIX),
    socket.socket(family=socket.AF_UNIX),
    socket.socket(family=socket.AF_UNIX)
]

input()
streams[0].connect("the_socket")
print("Stream 0 connected")

input()
streams[0].send(b"basic runner")
print("Stream 0 introduces")

input()
streams[1].connect("the_socket")
print("Stream 1 connected")

input()
streams[0].send(b"342")
print("Stream 0 sends messages")

input()
streams[1].send(b"the whistler")
print("Stream 1 introduces")

input()
streams[0].send(b"FINISH")
print("Stream 0 disconnects")

input()
streams[1].send(b"FINISH")
print("Stream 1 disconnects")
