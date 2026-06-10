import * as React from 'react'
import * as SliderPrimitive from '@radix-ui/react-slider'
import { cn } from '@/lib/utils'

export function Slider({ className, ...props }: React.ComponentPropsWithoutRef<typeof SliderPrimitive.Root>) {
  return (
    <SliderPrimitive.Root
      className={cn('relative flex w-full touch-none select-none items-center cursor-pointer', className)}
      {...props}
    >
      <SliderPrimitive.Track className="relative h-1.5 w-full grow overflow-hidden rounded-full bg-coal-700">
        <SliderPrimitive.Range className="absolute h-full bg-brass-500" />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb className="block size-4 rounded-full border-2 border-brass-400 bg-coal-900 shadow focus:outline-none focus-visible:outline-2 focus-visible:outline-brass-300" />
    </SliderPrimitive.Root>
  )
}
