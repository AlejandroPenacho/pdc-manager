import socket

stream = socket.socket(family=socket.AF_UNIX)

stream.connect("the_socket")

print(stream.recv(500))

while True:
    msg = input()
    stream.send(msg.encode())
