using System.Net;
using System.Net.Sockets;

namespace Midgard.Core;

/// <summary>
/// Free TCP ports on the loopback interface, for the services the shell starts. The port
/// of apps/mac/Sources/MidgardShellCore/Ports.swift.
/// </summary>
public static class Ports
{
    /// <summary>
    /// <paramref name="count"/> distinct free ports on 127.0.0.1. Every probe socket stays
    /// bound until all ports are chosen, so the system cannot hand out the same port twice;
    /// they are closed before returning. (A port may still be taken by someone else
    /// afterwards; the supervisor treats a failed bind as a service start failure.)
    /// </summary>
    /// <exception cref="SocketException">A probe socket could not be made or bound.</exception>
    public static IReadOnlyList<ushort> Free(int count)
    {
        var sockets = new List<Socket>();
        var ports = new List<ushort>();
        try
        {
            for (var i = 0; i < Math.Max(0, count); i++)
            {
                var socket = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
                sockets.Add(socket);
                socket.Bind(new IPEndPoint(IPAddress.Loopback, 0));
                ports.Add((ushort)((IPEndPoint)socket.LocalEndPoint!).Port);
            }
        }
        finally
        {
            foreach (var socket in sockets) socket.Dispose();
        }
        return ports;
    }
}
