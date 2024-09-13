import socket
import random
import time

class FakeTraining:
    def __init__(self, name):
        self.socket = socket.socket(family=socket.AF_UNIX)
        self.total_epochs = 1000
        self.current_epoch = 0
        self.connected = False
        self.name = name

    def progress(self):
        if not self.connected:
            self.socket.connect("the_socket")
            self.socket.send(self.name.encode())
            self.connected = True
            return False

        if self.current_epoch == self.total_epochs:
            self.socket.send(b"FINISH")
            return True

        self.current_epoch += random.randint(50, 150)
        self.current_epoch = min(self.current_epoch, self.total_epochs)
        message = "Hello with {self.current_epoch}"
        self.socket.send(f"{self.current_epoch}|{self.total_epochs}|{message}".encode())
        return False


all_trainings = [
    FakeTraining("Jordi Wild"),
    FakeTraining("El Bromas"),
    FakeTraining("Tommitllo")
]

while len(all_trainings) != 0:
    time.sleep(0.5)
    index = random.randint(0, len(all_trainings)-1)
    remove = all_trainings[index].progress()
    if remove:
        all_trainings.pop(index)
