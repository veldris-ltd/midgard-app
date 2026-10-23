using System.Net;
using System.Net.Sockets;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/PortsTests.swift.
public class PortsTests
{
    [Fact]
    public void PortsAreDistinctAndNonZero()
    {
        var ports = Ports.Free(8);
        Assert.Equal(8, ports.Count);
        Assert.Equal(8, ports.Distinct().Count());
        Assert.DoesNotContain((ushort)0, ports);
    }

    [Fact]
    public void ZeroCountIsEmpty()
    {
        Assert.Empty(Ports.Free(0));
    }

    [Fact]
    public void EachPortCanBeBoundAfterwards()
    {
        foreach (var port in Ports.Free(4))
        {
            using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            socket.Bind(new IPEndPoint(IPAddress.Loopback, port));
            Assert.Equal((int)port, ((IPEndPoint)socket.LocalEndPoint!).Port);
        }
    }
}
