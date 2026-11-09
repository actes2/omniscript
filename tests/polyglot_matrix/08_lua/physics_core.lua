function compute_momentum(mass, velocity)
    return mass * velocity
end

function compute_kinetic_energy(mass, velocity)
    return (mass * velocity * velocity) / 2
end
